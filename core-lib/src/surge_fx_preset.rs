//! Surge XT Effects の factory preset（`.srgfx`）を plugin state に組み立てる。
//!
//! plugin は preset-load 拡張を持たず、`.srgfx` を読む入口は GUI にしかない。
//! 一方 `clap.state` は JUCE の XML（[`juce_xml`]）で、effect 種別 `fxt` と
//! parameter を受け取る。したがって preset → state の変換をホスト側で行う。
//!
//! 変換で埋めなければならない差は 2 つ:
//!
//! 1. **並び**: `.srgfx` の `pN` は storage 順（effect の enum 順）、state の
//!    `fxp_N` は GUI 順（`posy_offset` による並べ替え、[`param_layout`]）
//! 2. **値域**: `.srgfx` は実値（dB や秒の log2 など）、state の `fxp_N` は
//!    0..1 に正規化した値。正規化の範囲は effect 種別と parameter ごとに違い、
//!    plugin にしか無いので、**plugin の自己申告から測る**（[`SurgeFxParamRanges`]）。
//!    plugin は state を保存するとき `surgeval_N` に実値を書くので、全 parameter を
//!    0 にした state と 1 にした state を読ませて保存させれば、下限と上限が分かる
//!
//! `vt_int` の parameter だけは state の `surgeval_N` を整数のまま読むので、
//! 正規化せずそのまま書く。

pub mod juce_xml;
mod param_layout;
mod streaming_migration;

use std::io::Cursor;

use anyhow::{bail, Context, Result};
use xmltree::{Element, EmitterConfig, XMLNode};

pub use param_layout::{SurgeFxParamLayout, SURGE_FX_PARAM_LAYOUTS};
pub use streaming_migration::{apply_streaming_migrations, SURGE_STREAMING_REVISION};

/// Surge XT Effects の CLAP plugin ID。
pub const SURGE_FX_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt-fx";

/// Surge の `n_fx_params`。
pub const SURGE_FX_PARAM_COUNT: usize = 12;

/// Surge XT Effects の plugin state が名乗る streaming version。
const STATE_STREAMING_VERSION: i32 = 2;

/// Surge `valtypes`。
pub const VALTYPE_INT: i32 = 0;
pub const VALTYPE_BOOL: i32 = 1;
pub const VALTYPE_FLOAT: i32 = 2;

/// state の `fxp_param_features_N` の bit。
const FEATURE_TEMPOSYNC: i32 = 1 << 0;
const FEATURE_EXTENDED: i32 = 1 << 1;
const FEATURE_DEACTIVATED: i32 = 1 << 3;

/// `.srgfx` の `<snapshot>` 1 件ぶんの parameter。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurgeFxParam {
    /// `pN`。無ければ preset は 0 を意味する（Surge の loader と同じ）。
    pub value: Option<f64>,
    pub temposync: bool,
    pub extend_range: bool,
    pub deactivated: bool,
    /// `pN_deform_type`。state には流せない。
    pub deform_type: Option<i32>,
}

impl SurgeFxParam {
    /// Surge の loader は属性が無い parameter に 0 を書く。
    pub fn raw_value(&self) -> f64 {
        self.value.unwrap_or(0.0)
    }
}

/// `.srgfx` の `<snapshot>` 1 件。
#[derive(Clone, Debug, PartialEq)]
pub struct SurgeFxSnapshot {
    pub name: String,
    /// `type`（Surge の `fx_type`）。
    pub fx_type: i32,
    pub streaming_version: i32,
    pub params: [SurgeFxParam; SURGE_FX_PARAM_COUNT],
}

/// `.srgfx` を読み、ファイル内の snapshot を並び順のまま返す。
///
/// factory preset には `<single-fx>` 文書を複数つなげたファイルがあるが、Surge 自身も
/// 先頭の文書しか読まないので、ここでも先頭の文書だけを見る。
///
/// 古い `streaming_version` の preset は Surge の loader と同じ移行
/// （[`apply_streaming_migrations`]）を済ませてから返す。
pub fn parse_srgfx(xml: &str) -> Result<Vec<SurgeFxSnapshot>> {
    let root = Element::parse(Cursor::new(xml.as_bytes())).context(".srgfx の XML が不正")?;
    if root.name != "single-fx" {
        bail!(
            ".srgfx の root element が single-fx ではない: '{}'",
            root.name
        );
    }
    let streaming_version = int_attribute(&root, "streaming_version").unwrap_or(0);
    let mut snapshots = Vec::new();
    for node in &root.children {
        let XMLNode::Element(snapshot) = node else {
            continue;
        };
        if snapshot.name != "snapshot" {
            continue;
        }
        let mut parsed = parse_snapshot(snapshot, streaming_version)?;
        apply_streaming_migrations(&mut parsed);
        snapshots.push(parsed);
    }
    if snapshots.is_empty() {
        bail!(".srgfx に snapshot が 1 件も無い");
    }
    Ok(snapshots)
}

fn parse_snapshot(element: &Element, streaming_version: i32) -> Result<SurgeFxSnapshot> {
    let name = element
        .attributes
        .get("name")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("snapshot に name が無い"))?;
    let fx_type = int_attribute(element, "type")
        .ok_or_else(|| anyhow::anyhow!("snapshot '{name}' に type が無い"))?;
    let mut params = [SurgeFxParam::default(); SURGE_FX_PARAM_COUNT];
    for (index, param) in params.iter_mut().enumerate() {
        let key = format!("p{index}");
        param.value = float_attribute(element, &key)
            .with_context(|| format!("snapshot '{name}' の {key} が数値ではない"))?;
        param.temposync = flag_attribute(element, &format!("{key}_temposync"));
        param.extend_range = flag_attribute(element, &format!("{key}_extend_range"));
        param.deactivated = flag_attribute(element, &format!("{key}_deactivated"));
        param.deform_type =
            float_attribute(element, &format!("{key}_deform_type"))?.map(|value| value as i32);
    }
    Ok(SurgeFxSnapshot {
        name,
        fx_type,
        streaming_version,
        params,
    })
}

fn int_attribute(element: &Element, key: &str) -> Option<i32> {
    element.attributes.get(key)?.trim().parse().ok()
}

fn float_attribute(element: &Element, key: &str) -> Result<Option<f64>> {
    element
        .attributes
        .get(key)
        .map(|text| {
            text.trim()
                .parse::<f64>()
                .with_context(|| format!("{key}=\"{text}\""))
        })
        .transpose()
}

fn flag_attribute(element: &Element, key: &str) -> bool {
    float_attribute(element, key)
        .ok()
        .flatten()
        .is_some_and(|value| value != 0.0)
}

/// effect 種別の layout。未知の種別は `None`。
pub fn param_layout(fx_type: i32) -> Option<&'static SurgeFxParamLayout> {
    SURGE_FX_PARAM_LAYOUTS
        .iter()
        .find(|layout| layout.fx_type == fx_type)
}

/// GUI 順（state の `fxp_N`）→ storage 順（`.srgfx` の `pN`）の対応。
///
/// plugin の `reorderSurgeParams()` と同じ規則: `posy_offset` が 0 でなく `ct_none` でも
/// ない parameter は `2 * index + posy_offset` の昇順、それ以外は末尾に元の順で並ぶ。
pub fn param_remap(layout: &SurgeFxParamLayout) -> [usize; SURGE_FX_PARAM_COUNT] {
    let mut order: Vec<(i64, usize)> = (0..SURGE_FX_PARAM_COUNT)
        .map(|index| {
            let key = if layout.posy_offset[index] != 0 && layout.active[index] {
                2 * index as i64 + i64::from(layout.posy_offset[index])
            } else {
                10_000
            };
            (key, index)
        })
        .collect();
    order.sort_by_key(|(key, _)| *key);
    let mut remap = [0; SURGE_FX_PARAM_COUNT];
    for (slot, (_, index)) in remap.iter_mut().zip(order) {
        *slot = index;
    }
    remap
}

/// plugin が保存した state XML から読み取った、GUI 順の parameter 一覧。
#[derive(Clone, Debug, PartialEq)]
pub struct SurgeFxStateReport {
    pub fx_type: i32,
    pub valtype: [i32; SURGE_FX_PARAM_COUNT],
    /// `surgeval_N`（実値）。
    pub value: [f64; SURGE_FX_PARAM_COUNT],
    pub features: [i32; SURGE_FX_PARAM_COUNT],
}

/// plugin state の XML（`<surgefx …/>`）を読む。
pub fn parse_state_xml(xml: &str) -> Result<SurgeFxStateReport> {
    let root = Element::parse(Cursor::new(xml.as_bytes())).context("surgefx state XML が不正")?;
    if root.name != "surgefx" {
        bail!("state の root element が surgefx ではない: '{}'", root.name);
    }
    let fx_type =
        int_attribute(&root, "fxt").ok_or_else(|| anyhow::anyhow!("state に fxt が無い"))?;
    let mut report = SurgeFxStateReport {
        fx_type,
        valtype: [VALTYPE_FLOAT; SURGE_FX_PARAM_COUNT],
        value: [0.0; SURGE_FX_PARAM_COUNT],
        features: [0; SURGE_FX_PARAM_COUNT],
    };
    for index in 0..SURGE_FX_PARAM_COUNT {
        report.valtype[index] =
            int_attribute(&root, &format!("surgevaltype_{index}")).unwrap_or(VALTYPE_FLOAT);
        report.value[index] = float_attribute(&root, &format!("surgeval_{index}"))?.unwrap_or(0.0);
        report.features[index] =
            int_attribute(&root, &format!("fxp_param_features_{index}")).unwrap_or(0);
    }
    Ok(report)
}

/// GUI 順の各 parameter について、正規化 0 / 1 に対応する実値。
#[derive(Clone, Debug, PartialEq)]
pub struct SurgeFxParamRanges {
    pub fx_type: i32,
    pub valtype: [i32; SURGE_FX_PARAM_COUNT],
    pub at_zero: [f64; SURGE_FX_PARAM_COUNT],
    pub at_one: [f64; SURGE_FX_PARAM_COUNT],
}

impl SurgeFxParamRanges {
    /// 「全 parameter を `normalized` にした state」を plugin に読ませ、保存させた結果 2 つから組む。
    pub fn from_reports(at_zero: &SurgeFxStateReport, at_one: &SurgeFxStateReport) -> Result<Self> {
        if at_zero.fx_type != at_one.fx_type {
            bail!(
                "range 計測の fxt が食い違う: {} と {}",
                at_zero.fx_type,
                at_one.fx_type
            );
        }
        if at_zero.valtype != at_one.valtype {
            bail!("range 計測の valtype が食い違う (fxt={})", at_zero.fx_type);
        }
        Ok(Self {
            fx_type: at_zero.fx_type,
            valtype: at_zero.valtype,
            at_zero: at_zero.value,
            at_one: at_one.value,
        })
    }

    fn normalize(&self, gui_index: usize, raw: f64) -> f64 {
        let span = self.at_one[gui_index] - self.at_zero[gui_index];
        if span == 0.0 {
            0.0
        } else {
            (raw - self.at_zero[gui_index]) / span
        }
    }
}

/// range 計測用の state。`fxt` を指定し、全 parameter の `fxp_N` を `normalized` にする。
pub fn calibration_state_xml(fx_type: i32, normalized: f64) -> String {
    let mut root = state_root(fx_type);
    for index in 0..SURGE_FX_PARAM_COUNT {
        root.attributes
            .insert(format!("fxp_{index}"), normalized.to_string());
    }
    emit(root)
}

/// snapshot を plugin state の XML にする。
pub fn snapshot_state_xml(
    snapshot: &SurgeFxSnapshot,
    layout: &SurgeFxParamLayout,
    ranges: &SurgeFxParamRanges,
) -> Result<String> {
    if layout.fx_type != snapshot.fx_type || ranges.fx_type != snapshot.fx_type {
        bail!(
            "snapshot '{}' の type {} と layout {} / ranges {} が食い違う",
            snapshot.name,
            snapshot.fx_type,
            layout.fx_type,
            ranges.fx_type
        );
    }
    let remap = param_remap(layout);
    let mut root = state_root(snapshot.fx_type);
    for (gui_index, storage_index) in remap.into_iter().enumerate() {
        let param = &snapshot.params[storage_index];
        let raw = param.raw_value();
        let valtype = ranges.valtype[gui_index];
        root.attributes
            .insert(format!("surgevaltype_{gui_index}"), valtype.to_string());
        if valtype == VALTYPE_INT {
            root.attributes
                .insert(format!("surgeval_{gui_index}"), (raw as i64).to_string());
            root.attributes
                .insert(format!("fxp_{gui_index}"), "0".to_string());
        } else {
            let normalized = ranges.normalize(gui_index, raw);
            root.attributes
                .insert(format!("fxp_{gui_index}"), normalized.to_string());
        }
        let mut features = 0;
        if param.temposync {
            features |= FEATURE_TEMPOSYNC;
        }
        if param.extend_range {
            features |= FEATURE_EXTENDED;
        }
        if param.deactivated {
            features |= FEATURE_DEACTIVATED;
        }
        root.attributes.insert(
            format!("fxp_param_features_{gui_index}"),
            features.to_string(),
        );
    }
    Ok(emit(root))
}

fn state_root(fx_type: i32) -> Element {
    let mut root = Element::new("surgefx");
    root.attributes.insert(
        "streamingVersion".to_string(),
        STATE_STREAMING_VERSION.to_string(),
    );
    root.attributes
        .insert("fxt".to_string(), fx_type.to_string());
    root
}

fn emit(root: Element) -> String {
    let mut out = Vec::new();
    root.write_with_config(
        &mut out,
        EmitterConfig::new()
            .write_document_declaration(true)
            .perform_indent(false),
    )
    .expect("Vec<u8> への XML 書き出しは失敗しない");
    String::from_utf8(out).expect("xmltree は UTF-8 を書く")
}

#[cfg(test)]
mod tests;
