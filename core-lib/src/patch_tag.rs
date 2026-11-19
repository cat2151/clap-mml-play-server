//! patch 文字列 `<パス><拡張子>:<タグ>` の解析と組み立て。
//!
//! 拡張子は loader（[`PatchForm`]）を、タグは所有者（routing と基点）を決める。`.h2p` のように
//! 複数の plugin が同じ拡張子を使うため、拡張子だけでは送り先が決まらない。
//! 正規形は 1 つ: 既定所有者は空タグだけで書き、既定所有者の名前をタグに書くのはエラー。
//!
//! タグ剥離はここの [`parse_patch_wire`] だけで行い、`Path`・`PatchBase`・`is_*_patch_path` には
//! 剥離後の文字列だけを渡す。Windows の `Path` は `:` を ADS 区切りとして扱い、`extension()` が崩れる。

use std::fmt;

use cmrt_server_config::{PatchForm, ZEBRALETTE3_PATCH_TAG};

use crate::{
    cache_wav::is_cache_wav_patch_path, is_cartridge_patch_path, is_floe_preset_path,
    is_h2p_patch_path, is_sforzando_patch_path, is_six_sines_patch_path, is_vvp_patch_path,
};

/// タグの上限文字数。
pub const MAX_PATCH_TAG_LEN: usize = 16;

/// (拡張子の形, タグ) → 所有者 の 1 行。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchTagRow {
    pub form: PatchForm,
    pub tag: &'static str,
    /// エラー文で所有者を名指すための名前。routing には使わない。
    pub owner: &'static str,
}

/// 既定所有者（空タグ）。全 [`PatchForm`] がちょうど 1 回現れる。
const DEFAULT_OWNERS: &[PatchTagRow] = &[
    row(PatchForm::StateFile, "Surge XT"),
    row(PatchForm::Cartridge, "Dexed"),
    row(PatchForm::Vvp, "Vaporizer2"),
    row(PatchForm::FloePreset, "Floe"),
    row(PatchForm::Sfz, "sforzando"),
    row(PatchForm::SixSines, "Six Sines"),
    row(PatchForm::H2p, "TyrellN6"),
    row(PatchForm::CacheWav, "cache-player"),
];

/// 共有拡張子の追加タグ。既定所有者以外が同じ拡張子を読むときだけ行を足す。
const EXTRA_TAGS: &[PatchTagRow] = &[PatchTagRow {
    form: PatchForm::H2p,
    tag: ZEBRALETTE3_PATCH_TAG,
    owner: "Zebralette3",
}];

const fn row(form: PatchForm, owner: &'static str) -> PatchTagRow {
    PatchTagRow {
        form,
        tag: "",
        owner,
    }
}

fn rows() -> impl Iterator<Item = &'static PatchTagRow> {
    DEFAULT_OWNERS.iter().chain(EXTRA_TAGS)
}

/// 境界で解析した patch 文字列。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchWire<'a> {
    /// タグを剥がしたパス。`Path` や基点の解決へはこれだけを渡す。
    pub path: &'a str,
    pub tag: &'a str,
    pub form: PatchForm,
    pub owner: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatchWireError {
    /// 区切りの `:` が無い。
    Legacy { patch: String },
    /// タグが `[a-zA-Z0-9]*` でない、または長すぎる。
    InvalidTag { patch: String, tag: String },
    /// 拡張子から loader が決まらない。
    UnknownExtension { patch: String },
    /// その拡張子にそのタグの所有者がいない。
    UnknownTag {
        patch: String,
        tag: String,
        valid: Vec<PatchTagRow>,
    },
}

impl fmt::Display for PatchWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Legacy { patch } => write!(
                f,
                "音色の指定にタグの区切り ':' がありません（旧形式）: '{patch}'。末尾に ':' を付けてください（例 '{patch}:'）"
            ),
            Self::InvalidTag { patch, tag } => write!(
                f,
                "音色のタグ '{tag}' が不正です: '{patch}'（英数字 {MAX_PATCH_TAG_LEN} 文字以内、空可）"
            ),
            Self::UnknownExtension { patch } => {
                write!(f, "音色の拡張子を読めるプラグインがありません: '{patch}'")
            }
            Self::UnknownTag { patch, tag, valid } => {
                let choices = valid
                    .iter()
                    .map(|row| format!("{} は ':{}'", row.owner, row.tag))
                    .collect::<Vec<_>>()
                    .join("、");
                write!(
                    f,
                    "音色のタグ '{tag}' はこの拡張子に登録されていません: '{patch}'（{choices}）"
                )
            }
        }
    }
}

impl std::error::Error for PatchWireError {}

/// wire の patch 文字列をパスとタグに分け、所有者を表から引く。
///
/// 末尾の `:` で切る。`:` の後ろにパス区切りがあれば、それはドライブ文字の `:` なので
/// タグの区切りが無い（旧形式）とみなす。
pub fn parse_patch_wire(wire: &str) -> Result<PatchWire<'_>, PatchWireError> {
    let Some((path, tag)) = split_wire(wire) else {
        return Err(PatchWireError::Legacy {
            patch: wire.to_string(),
        });
    };
    if tag.len() > MAX_PATCH_TAG_LEN || !tag.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(PatchWireError::InvalidTag {
            patch: wire.to_string(),
            tag: tag.to_string(),
        });
    }
    let form = patch_form_of_path(path).ok_or_else(|| PatchWireError::UnknownExtension {
        patch: wire.to_string(),
    })?;
    let row = rows()
        .find(|row| row.form == form && row.tag == tag)
        .ok_or_else(|| PatchWireError::UnknownTag {
            patch: wire.to_string(),
            tag: tag.to_string(),
            valid: rows().filter(|row| row.form == form).copied().collect(),
        })?;
    Ok(PatchWire {
        path,
        tag,
        form,
        owner: row.owner,
    })
}

/// 末尾の `:` でパスとタグに分ける。`:` の後ろにパス区切りがあればドライブ文字なので `None`。
fn split_wire(wire: &str) -> Option<(&str, &str)> {
    wire.rsplit_once(':')
        .filter(|(_, tag)| !tag.contains(['/', '\\']))
}

impl PatchWire<'_> {
    /// パス部分だけを差し替えた wire（基点で絶対パスへ直したとき・表示名へ直したとき）。
    pub fn with_path(&self, path: &str) -> String {
        format!("{path}:{}", self.tag)
    }
}

/// タグを持たないパス（ランダム選択・cache WAV・既定所有者向けの内部経路）を、
/// 既定所有者の wire にする。
pub fn default_owner_wire(path: &str) -> String {
    format!("{path}:")
}

/// MML 先頭 JSON の音色キー。
pub const PATCH_JSON_KEY: &str = "patch";

/// MML 先頭 JSON の、音色一覧の絞り込み文字列のキー。server は読まず、client が書き戻す。
pub const PATCH_FILTER_QUERY_JSON_KEY: &str = "patch filter";

/// (改名前のキー, 今のキー)。旧キーが残っていたら既定音色で鳴らさずエラーにする。
const LEGACY_PATCH_JSON_KEYS: [(&str, &str); 2] = [
    ("Surge XT patch", PATCH_JSON_KEY),
    ("Surge XT patch filter", PATCH_FILTER_QUERY_JSON_KEY),
];

/// MML 先頭 JSON から音色の wire を取り出す。キーが無ければ `None`、旧キーがあればエラー。
pub fn patch_from_embedded_json(value: &serde_json::Value) -> Result<Option<&str>, String> {
    if let Some((old, new)) = LEGACY_PATCH_JSON_KEYS
        .iter()
        .find(|(old, _)| value.get(*old).is_some())
    {
        return Err(format!(
            "MML 先頭 JSON の '{old}' は旧キーです。'{new}' に書き換えてください"
        ));
    }
    Ok(value
        .get(PATCH_JSON_KEY)
        .and_then(serde_json::Value::as_str))
}

/// 表示名（空タグの `:` を落とした形）から wire を作る。
///
/// 末尾が登録済みの非空タグならそのまま、そうでなければ空タグの `:` を足す。
pub fn wire_from_display(display: &str) -> String {
    if split_display(display).1.is_empty() {
        format!("{display}:")
    } else {
        display.to_string()
    }
}

/// 表示名をパスとタグに分ける。末尾が登録済みの非空タグでなければ、全体がパスで空タグ。
pub fn split_display(display: &str) -> (&str, &str) {
    display
        .rsplit_once(':')
        .filter(|(path, tag)| {
            !tag.is_empty()
                && patch_form_of_path(path)
                    .is_some_and(|form| EXTRA_TAGS.iter().any(|r| r.form == form && r.tag == *tag))
        })
        .unwrap_or((display, ""))
}

/// wire から表示名を作る。空タグなら `:` を落とし、非空タグは残す。
pub fn display_from_wire(wire: &PatchWire<'_>) -> String {
    if wire.tag.is_empty() {
        wire.path.to_string()
    } else {
        format!("{}:{}", wire.path, wire.tag)
    }
}

/// wire から表示名を作る。表から所有者を引かないので、拡張子やタグが未登録でも
/// 区切りの `:` さえあれば表示名になる（妥当性は鳴らす側の [`parse_patch_wire`] が見る）。
/// 区切りが無い旧形式は `None`。
pub fn display_from_wire_str(wire: &str) -> Option<String> {
    let (path, tag) = split_wire(wire)?;
    Some(if tag.is_empty() {
        path.to_string()
    } else {
        wire.to_string()
    })
}

/// MML 先頭 JSON の音色を表示名で返す。キーが無い・旧キー・旧形式は `None`。
pub fn patch_display_from_embedded_json(value: &serde_json::Value) -> Option<String> {
    display_from_wire_str(patch_from_embedded_json(value).ok()??)
}

/// タグを剥がした patch パスの拡張子から loader を決める。未知の拡張子は `None`。
pub(crate) fn patch_form_of_path(patch: &str) -> Option<PatchForm> {
    let form = if is_cartridge_patch_path(patch) {
        PatchForm::Cartridge
    } else if is_sforzando_patch_path(patch) {
        PatchForm::Sfz
    } else if is_floe_preset_path(patch) {
        PatchForm::FloePreset
    } else if is_vvp_patch_path(patch) {
        PatchForm::Vvp
    } else if is_six_sines_patch_path(patch) {
        PatchForm::SixSines
    } else if is_h2p_patch_path(patch) {
        PatchForm::H2p
    } else if is_cache_wav_patch_path(patch) {
        PatchForm::CacheWav
    } else if is_state_file_patch_path(patch) {
        PatchForm::StateFile
    } else {
        return None;
    };
    Some(form)
}

fn is_state_file_patch_path(patch: &str) -> bool {
    const FXP: &str = ".fxp";
    patch.len() > FXP.len()
        && patch
            .get(patch.len() - FXP.len()..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(FXP))
}

#[cfg(test)]
mod tests;
