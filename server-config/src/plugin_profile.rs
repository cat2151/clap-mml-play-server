//! `[plugins.*]` と組み込み値によるプラグインプロファイル。
//!
//! 既知のプラグインは [`builtin_plugin_profiles`] に組み込みで持っている。
//! 標準の場所へインストールしてあるなら `[plugins.*]` を書く必要はなく、標準値との差分だけを
//! 同名の table で上書きできる。
//!
//! 「どこにプラグインがあるか」「そのプラグインの音色置き場はどう組まれているか」は
//! プラグインをロードする側の知識なので、この解決規則は play server repo が持つ。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{
    default_dexed_cartridge_dirs, default_dexed_plugin_path, default_floe_plugin_path,
    default_jc303_plugin_path, default_patches_dirs, default_plugin_path,
    default_sforzando_plugin_path, default_six_sines_plugin_path, default_tyrelln6_plugin_path,
    default_vaporizer2_plugin_path, default_zebralette3_plugin_path, u_he_product_of,
    CACHE_PLAYER_PLUGIN_ID, DEXED_PLUGIN_ID, FLOE_PLUGIN_ID, JC303_PLUGIN_ID, SFORZANDO_PLUGIN_ID,
    SIX_SINES_PLUGIN_ID, SURGE_XT_PLUGIN_ID, TYRELLN6_PLUGIN_ID, VAPORIZER2_PLUGIN_ID,
    ZEBRALETTE3_PLUGIN_ID,
};

/// `[plugins.<名前>]` 1 つ分のプラグイン設定。
///
/// 各項目は「書かなければ組み込みプロファイルの値を引き継ぐ」。`patches_dirs` を
/// 明示的に空にしたいときは `patches_dirs = []` と書く。
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginProfile {
    #[serde(default)]
    pub plugin_path: String,
    /// 期待する CLAP plugin ID。診断用の任意項目。
    #[serde(default)]
    pub plugin_id: Option<String>,
    /// このプラグインの音色置き場。
    #[serde(default)]
    pub patches_dirs: Option<Vec<String>>,
}

impl PluginProfile {
    /// `self` を土台に `over` の「書かれている項目」だけを上書きする。
    fn overridden_by(self, over: PluginProfile) -> Self {
        Self {
            plugin_path: if over.plugin_path.trim().is_empty() {
                self.plugin_path
            } else {
                over.plugin_path
            },
            plugin_id: over.plugin_id.or(self.plugin_id),
            patches_dirs: over.patches_dirs.or(self.patches_dirs),
        }
    }
}

/// config に何も書かなくても使える組み込みプロファイル。
///
/// パスは OS ごとの標準インストール先（[`default_plugin_path`] などと同じ根拠）。
/// 別の場所に入れている場合だけ `[plugins.<名前>]` に `plugin_path` を書けばよく、
/// `plugin_id` や `patches_dirs` はここの値が引き継がれる。
pub fn builtin_plugin_profiles() -> BTreeMap<String, PluginProfile> {
    BTreeMap::from([
        (
            "Apricot".to_string(),
            PluginProfile {
                plugin_path: if cfg!(windows) {
                    r"C:\Program Files\Common Files\CLAP\Apricot.clap".into()
                } else {
                    String::new()
                },
                plugin_id: Some(crate::APRICOT_PLUGIN_ID.into()),
                patches_dirs: Some(plugin_presets::apricot::apricot_preset_dirs()),
            },
        ),
        (
            "OB-Xf".to_string(),
            PluginProfile {
                plugin_path: if cfg!(windows) {
                    r"C:\Program Files\Common Files\CLAP\Surge Synth Team\OB-Xf.clap".to_string()
                } else {
                    String::new()
                },
                plugin_id: Some(crate::OBXF_PLUGIN_ID.to_string()),
                patches_dirs: Some(plugin_presets::obxf::obxf_preset_dirs()),
            },
        ),
        (
            "Surge XT".to_string(),
            PluginProfile {
                plugin_path: default_plugin_path().to_string(),
                plugin_id: Some(SURGE_XT_PLUGIN_ID.to_string()),
                patches_dirs: Some(default_patches_dirs()),
            },
        ),
        (
            "Dexed".to_string(),
            PluginProfile {
                plugin_path: default_dexed_plugin_path().to_string(),
                plugin_id: Some(DEXED_PLUGIN_ID.to_string()),
                patches_dirs: Some(default_dexed_cartridge_dirs()),
            },
        ),
        (
            "Vaporizer2".to_string(),
            PluginProfile {
                plugin_path: default_vaporizer2_plugin_path().to_string(),
                plugin_id: Some(VAPORIZER2_PLUGIN_ID.to_string()),
                // 音色置き場は registry から [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
        (
            "Floe".to_string(),
            PluginProfile {
                plugin_path: default_floe_plugin_path().to_string(),
                plugin_id: Some(FLOE_PLUGIN_ID.to_string()),
                // 音色置き場は floe.ini から [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
        (
            "Sforzando".to_string(),
            PluginProfile {
                plugin_path: default_sforzando_plugin_path().to_string(),
                plugin_id: Some(SFORZANDO_PLUGIN_ID.to_string()),
                // 音色置き場は ARIA の registry だけから決まる。config の値は使わない。
                patches_dirs: None,
            },
        ),
        (
            "Six Sines".to_string(),
            PluginProfile {
                plugin_path: default_six_sines_plugin_path().to_string(),
                // 同じ `.clap` に descriptor が 2 つあり、ID が無いと選べない。
                plugin_id: Some(SIX_SINES_PLUGIN_ID.to_string()),
                // 音色置き場は factory の取得先を [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
        (
            "TyrellN6".to_string(),
            PluginProfile {
                plugin_path: default_tyrelln6_plugin_path().to_string(),
                plugin_id: Some(TYRELLN6_PLUGIN_ID.to_string()),
                // 音色置き場は registry から [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
        (
            "Zebralette3".to_string(),
            PluginProfile {
                plugin_path: default_zebralette3_plugin_path().to_string(),
                plugin_id: Some(ZEBRALETTE3_PLUGIN_ID.to_string()),
                // 音色置き場は registry から [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
        (
            "JC303".to_string(),
            PluginProfile {
                plugin_path: default_jc303_plugin_path().to_string(),
                plugin_id: Some(JC303_PLUGIN_ID.to_string()),
                // 音色置き場は本体の preset 置き場を [`merged_plugin_profiles`] が埋める。
                patches_dirs: None,
            },
        ),
    ])
}

/// このプラグインの音色が、patch 文字列としてどういう形で指されるか。
///
/// 拡張子から決まる loader の種類。送り先の plugin は、これと patch 文字列のタグの組で決まる
/// （`docs/adr/0026-patch-tag-decides-the-owner.md`）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchForm {
    /// ファイル 1 つ = 音色 1 つ。Surge XT の `.fxp`（CLAP state）。
    StateFile,
    /// cartridge ファイルの中の 1 program。Dexed の `.syx`。
    Cartridge,
    /// ファイル 1 つ = 音色 1 つ。Vaporizer2 の `.vvp`（XML を CLAP state へ包む）。
    ///
    /// 単位は [`PatchForm::StateFile`] と同じだが、**別の形として数える**。
    /// 一緒にしてしまうと Surge XT と Vaporizer2 のどちらへ送るべき patch なのかが
    /// 決まらず、片方の音色がもう片方のインスタンスへ流れる（ADR 0007 が
    /// 「承知したうえで受け入れた弱点」として挙げていた穴）。`.vvp` という固有拡張子が
    /// あるおかげで、patch 文字列を変えずに（＝永続 ID を壊さずに）分けられる。
    Vvp,
    /// ファイル 1 つ = 音色 1 つ。Floe の `.floe-preset`（Floe 固有 loader）。
    ///
    /// [`PatchForm::StateFile`] と分けることで、Surge XT の instance へ Floe preset を
    /// 誤投入しない。固有拡張子なので display 文字列を変えずに routing できる。
    FloePreset,
    /// ファイル 1 つ = 音色 1 つ。sforzando の `.sfz`（vendor state adapter）。
    Sfz,
    /// ファイル 1 つ = 音色 1 つ。Six Sines の `.sxsnp`（中身の XML をそのまま CLAP state へ）。
    ///
    /// Surge XT の `.fxp` と同じく state だが、固有拡張子で分けて互いの instance へ流さない。
    SixSines,
    /// ファイル 1 つ = 音色 1 つ。u-he の `.h2p`（バイト列をそのまま CLAP state へ）。
    ///
    /// TyrellN6 と Zebralette3 が同じ拡張子を読む。どちらへ送るかは patch タグで決める
    /// （[`crate::U_HE_PRODUCTS`]）。
    H2p,
    /// ファイル 1 つ = 音色 1 つ。JC303 の `.json`（init state を雛形に値を書き込んで CLAP state へ）。
    Jc303Preset,
    /// Apricot の `.synthpatch`。factory は bundle 以下の仮想パス。
    ApricotPreset,
    /// ファイル 1 つ = 音源 1 つ。組み込み cache-player の `.wav`（DAW の cell キャッシュ）。
    ///
    /// 他の形と違い、**音色ではなく録音済みの音そのもの**を指す。それでも patch 文字列の
    /// 形として数えるのは、live 経路の `PreparePatch` / `PrepareStandbyPatch` が
    /// 「patch 文字列 → プラグイン」の 1 本道でしか物理インスタンスを選べないため
    /// （`docs/adr/0007-patch-string-decides-the-plugin.md`）。
    CacheWav,
}

/// プロファイルが扱う patch 文字列の形。
///
/// cartridge を音色置き場にする既知のプラグインは Dexed だけ、`.vvp` は Vaporizer2 だけ。
/// `plugin_id` が書かれていない config でも判定できるよう、ファイル名も最後の手段として見る
/// （[`crate::plugin_file_stem`] と同じ考え方）。
///
/// 未知のプラグインが [`PatchForm::StateFile`] へ落ちるのは変えていない。`.fxp` を
/// 読む CLAP は他にもありうるが、`.syx` や `.vvp` を読むものは実質この 2 つだけなので、
/// 「知らないものは Surge と同じ形」という既定のほうが当たる見込みが高い。
pub fn patch_form_of(plugin_id: Option<&str>, plugin_path: &str) -> PatchForm {
    let matches = |id: &str, stem_keyword: &str| match plugin_id {
        Some(plugin_id) => plugin_id == id,
        None => crate::plugin_file_stem(plugin_path)
            .to_lowercase()
            .contains(stem_keyword),
    };
    if matches(CACHE_PLAYER_PLUGIN_ID, "cache-player") {
        PatchForm::CacheWav
    } else if matches(DEXED_PLUGIN_ID, "dexed") {
        PatchForm::Cartridge
    } else if matches(SFORZANDO_PLUGIN_ID, "sforzando") {
        PatchForm::Sfz
    } else if matches(FLOE_PLUGIN_ID, "floe") {
        PatchForm::FloePreset
    } else if matches(VAPORIZER2_PLUGIN_ID, "vaporizer") {
        PatchForm::Vvp
    } else if matches(SIX_SINES_PLUGIN_ID, "six sines") {
        PatchForm::SixSines
    } else if plugin_presets::apricot::is_apricot(plugin_id, plugin_path) {
        PatchForm::ApricotPreset
    } else if matches(JC303_PLUGIN_ID, "jc303") {
        PatchForm::Jc303Preset
    } else if u_he_product_of(plugin_id, plugin_path).is_some() {
        PatchForm::H2p
    } else {
        PatchForm::StateFile
    }
}

/// 組み込みプロファイルと config の `[plugins.*]` を合わせた一覧。
///
/// 表記ゆれを吸収し、同名なら config 側の「書かれている項目」が組み込みを上書きする。
///
/// Vaporizer2・Floe・u-he（TyrellN6・Zebralette3）・JC303 の `patches_dirs` は config を無視し、本体の
/// 設定や置き場から決めた値で置き換える（[`plugin_presets::vaporizer2::vaporizer2_preset_dirs`]・
/// [`plugin_presets::floe::floe_preset_dirs`]・[`plugin_presets::u_he::u_he_preset_dirs`]・
/// [`plugin_presets::jc303::jc303_preset_dirs`]）。
/// Six Sines は factory の取得先（[`crate::six_sines_factory_dir`]）で置き換える。
pub fn merged_plugin_profiles(
    from_config: &BTreeMap<String, PluginProfile>,
) -> BTreeMap<String, PluginProfile> {
    let mut merged = builtin_plugin_profiles();
    for (name, configured) in from_config {
        let base = lookup(&merged, name).unwrap_or_default();
        // 表記ゆれで組み込みに当たった場合、組み込み側のキー名へ寄せる。
        let key = merged
            .keys()
            .find(|candidate| normalized(candidate) == normalized(name))
            .cloned()
            .unwrap_or_else(|| name.clone());
        merged.insert(key, base.overridden_by(configured.clone()));
    }
    for profile in merged.values_mut() {
        if plugin_presets::obxf::is_obxf(profile.plugin_id.as_deref(), &profile.plugin_path) {
            profile.patches_dirs = Some(plugin_presets::obxf::obxf_preset_dirs());
        }
        match patch_form_of(profile.plugin_id.as_deref(), &profile.plugin_path) {
            PatchForm::ApricotPreset => {
                profile.patches_dirs = Some(plugin_presets::apricot::apricot_preset_dirs())
            }
            PatchForm::Vvp => {
                profile.patches_dirs = Some(plugin_presets::vaporizer2::vaporizer2_preset_dirs())
            }
            PatchForm::FloePreset => {
                profile.patches_dirs = Some(plugin_presets::floe::floe_preset_dirs())
            }
            PatchForm::H2p => {
                profile.patches_dirs =
                    u_he_product_of(profile.plugin_id.as_deref(), &profile.plugin_path)
                        .map(|product| plugin_presets::u_he::u_he_preset_dirs(product.product))
            }
            PatchForm::Jc303Preset => {
                profile.patches_dirs = Some(plugin_presets::jc303::jc303_preset_dirs())
            }
            PatchForm::SixSines => {
                profile.patches_dirs = Some(
                    crate::six_sines_factory_dir()
                        .map(|dir| dir.to_string_lossy().into_owned())
                        .into_iter()
                        .collect(),
                )
            }
            _ => {}
        }
    }
    merged
}

/// [`merged_plugin_profiles`] のうち、`plugin_path` が実在するものだけ。
///
/// 未インストールのプラグインを候補に残すと、インスタンスを作ろうとして初めて
/// 「ロードできない」で落ちる。**載せる前に存在を確かめる**
/// （`docs/adr/0008-spare-instance-pool.md`）。
pub fn installed_plugin_profiles(
    from_config: &BTreeMap<String, PluginProfile>,
) -> BTreeMap<String, PluginProfile> {
    merged_plugin_profiles(from_config)
        .into_iter()
        .filter(|(_, profile)| {
            let path = profile.plugin_path.trim();
            !path.is_empty() && Path::new(path).exists()
        })
        .collect()
}

/// 表記ゆれを吸収するための比較用キー。`Surge XT` / `surge_xt` / `SurgeXT` を同一視する。
fn normalized(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 完全一致を優先し、無ければ表記ゆれを吸収して探す。
fn lookup(profiles: &BTreeMap<String, PluginProfile>, name: &str) -> Option<PluginProfile> {
    if let Some(profile) = profiles.get(name) {
        return Some(profile.clone());
    }
    let key = normalized(name);
    profiles
        .iter()
        .find(|(candidate, _)| normalized(candidate) == key)
        .map(|(_, profile)| profile.clone())
}

#[cfg(test)]
mod tests;
