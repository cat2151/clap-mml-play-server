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
    default_patches_dirs, default_plugin_path, default_sforzando_plugin_path,
    default_vaporizer2_plugin_path, DEXED_PLUGIN_ID, FLOE_PLUGIN_ID, SFORZANDO_PLUGIN_ID,
    SURGE_XT_PLUGIN_ID, VAPORIZER2_PLUGIN_ID,
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
                // 音色置き場に既定値は無い（[`default_vaporizer2_plugin_path`] の理由）。
                // `None` は「書かれていない」なので、config の `[plugins.Vaporizer2]` に
                // `patches_dirs` を書けばそれがそのまま効く。書かなければ音色置き場が
                // 空のままカタログに載らない。
                patches_dirs: None,
            },
        ),
        (
            "Floe".to_string(),
            PluginProfile {
                plugin_path: default_floe_plugin_path().to_string(),
                plugin_id: Some(FLOE_PLUGIN_ID.to_string()),
                // preset library は環境依存なので config の `[plugins.Floe]` で指定する。
                patches_dirs: None,
            },
        ),
        (
            "Sforzando".to_string(),
            PluginProfile {
                plugin_path: default_sforzando_plugin_path().to_string(),
                plugin_id: Some(SFORZANDO_PLUGIN_ID.to_string()),
                // preset-discovery と config の和集合はカタログを組む時点で解決する。
                patches_dirs: None,
            },
        ),
    ])
}

/// このプラグインの音色が、patch 文字列としてどういう形で指されるか。
///
/// 1 プロセスに複数のプラグインを載せると、「この patch 文字列はどちらのプラグインの
/// ものか」を決める規則が要る。現状その材料は**文字列の形だけ**で、`.syx` コンポーネントを
/// 含むかどうかで割り切れている（`docs/adr/0007-patch-string-decides-the-plugin.md`）。
/// patch 文字列そのものにプラグイン名を入れる仕様変更は、display 文字列が永続 ID である
/// ため保存済みデータの移行が要る。いまは踏み込まない（同 §2.1）。
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
    if matches(DEXED_PLUGIN_ID, "dexed") {
        PatchForm::Cartridge
    } else if matches(SFORZANDO_PLUGIN_ID, "sforzando") {
        PatchForm::Sfz
    } else if matches(FLOE_PLUGIN_ID, "floe") {
        PatchForm::FloePreset
    } else if matches(VAPORIZER2_PLUGIN_ID, "vaporizer") {
        PatchForm::Vvp
    } else {
        PatchForm::StateFile
    }
}

/// 組み込みプロファイルと config の `[plugins.*]` を合わせた一覧。
///
/// 同名なら config 側の「書かれている項目」が組み込みを上書きする
/// 表記ゆれを吸収し、同名なら config 側の差分を優先する。
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
