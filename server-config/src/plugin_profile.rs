//! `active_plugin` + `[plugins.*]` によるプラグイン切り替え。
//!
//! 既知のプラグインは [`builtin_plugin_profiles`] に組み込みで持っている。
//! 標準の場所へインストールしてあるなら `active_plugin = 'Dexed'` の 1 行だけでよく、
//! `[plugins.*]` を書く必要はない。
//!
//! 解決結果（[`resolve_active_plugin_profile`] の戻り値）は、呼び出し側が自分の
//! config 構造体のトップレベルフィールドへ焼き込む。こうしておくと
//! `cfg.plugin_path` / `cfg.patches_dirs` の読み手（app・各サーバー）は
//! プロファイルの存在を一切知らずに済む。
//!
//! 「どこにプラグインがあるか」「そのプラグインの音色置き場はどう組まれているか」は
//! プラグインをロードする側の知識なので、この解決規則は play server repo が持つ。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{
    default_dexed_cartridge_dirs, default_dexed_plugin_path, default_patches_dirs,
    default_plugin_path, default_vaporizer2_plugin_path, DEXED_PLUGIN_ID, SURGE_XT_PLUGIN_ID,
    VAPORIZER2_PLUGIN_ID,
};

/// `[plugins.<名前>]` 1 つ分のプラグイン設定。
///
/// Surge XT / Dexed / Vaporizer2 を config に残したまま、`active_plugin` 1 行で
/// 行き来できるようにするためのもの。
///
/// 各項目は「書かなければ組み込みプロファイルの値を引き継ぐ」。`patches_dirs` を
/// 明示的に空にしたいときは `patches_dirs = []` と書く。
///
/// [`PatchRoleFilters`] の項目はトップレベルと同じキー名で、`[plugins.*]` の中へ
/// そのまま書ける。
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginProfile {
    #[serde(default)]
    pub plugin_path: String,
    /// 期待する CLAP plugin ID。診断用の任意項目。
    #[serde(default)]
    pub plugin_id: Option<String>,
    /// このプラグインの音色置き場。
    #[serde(default)]
    pub patches_dirs: Option<Vec<String>>,
    /// 用途別の patch 自動選択の絞り込み。トップレベルと同じキー名で書ける。
    ///
    /// この crate 自身は読まない（サーバーは patch の用途別自動選択をしない）。
    /// プラグインごとの正解は組み込みプロファイルが持つ知識なのでここで解決し、
    /// 使うのは TUI 側。
    #[serde(flatten)]
    pub patch_roles: PatchRoleFilters,
}

/// 用途別 patch 自動選択（grid sequencer の chord / bass / arpeggio / drum 行）の
/// 絞り込み設定のうち、プラグインごとに正解が違うもの。
///
/// TUI 側のトップレベル既定値は Surge のカテゴリ名なので、cartridge を音色置き場にする
/// Dexed では 1 つも当たらない。プラグインごとの正解をここへ持たせる。
///
/// 各項目は `None` が「書かれていない」で、そのとき呼び出し側のトップレベルの値を
/// そのまま使う。`[]` は「カテゴリで絞らない」という**明示の指定**なので区別が要る。
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchRoleFilters {
    #[serde(default)]
    pub chord_patch_categories: Option<Vec<String>>,
    #[serde(default)]
    pub bass_patch_categories: Option<Vec<String>>,
    #[serde(default)]
    pub arpeggio_patch_categories: Option<Vec<String>>,
    #[serde(default)]
    pub drum_patch_categories: Option<Vec<String>>,
    #[serde(default)]
    pub kick_patch_keywords: Option<Vec<String>>,
    #[serde(default)]
    pub snare_patch_keywords: Option<Vec<String>>,
    #[serde(default)]
    pub hihat_patch_keywords: Option<Vec<String>>,
}

impl PatchRoleFilters {
    /// どの項目でも絞らない設定。カテゴリ階層を持たない音色置き場のプラグイン用。
    ///
    /// cartridge のディレクトリ名（`SynprezFM` など）は用途と無関係なので、
    /// カテゴリで絞る前提そのものが成り立たない。絞らずに全 program を候補にする。
    pub fn unfiltered() -> Self {
        Self {
            chord_patch_categories: Some(Vec::new()),
            bass_patch_categories: Some(Vec::new()),
            arpeggio_patch_categories: Some(Vec::new()),
            drum_patch_categories: Some(Vec::new()),
            kick_patch_keywords: Some(Vec::new()),
            snare_patch_keywords: Some(Vec::new()),
            hihat_patch_keywords: Some(Vec::new()),
        }
    }

    /// `self` を土台に `over` の「書かれている項目」だけを上書きする。
    pub(crate) fn overridden_by(self, over: Self) -> Self {
        Self {
            chord_patch_categories: over.chord_patch_categories.or(self.chord_patch_categories),
            bass_patch_categories: over.bass_patch_categories.or(self.bass_patch_categories),
            arpeggio_patch_categories: over
                .arpeggio_patch_categories
                .or(self.arpeggio_patch_categories),
            drum_patch_categories: over.drum_patch_categories.or(self.drum_patch_categories),
            kick_patch_keywords: over.kick_patch_keywords.or(self.kick_patch_keywords),
            snare_patch_keywords: over.snare_patch_keywords.or(self.snare_patch_keywords),
            hihat_patch_keywords: over.hihat_patch_keywords.or(self.hihat_patch_keywords),
        }
    }
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
            patch_roles: self.patch_roles.overridden_by(over.patch_roles),
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
                // カテゴリ設定の TUI 側トップレベル既定値が Surge のものなので、書く必要が無い。
                patch_roles: PatchRoleFilters::default(),
            },
        ),
        (
            "Dexed".to_string(),
            PluginProfile {
                plugin_path: default_dexed_plugin_path().to_string(),
                plugin_id: Some(DEXED_PLUGIN_ID.to_string()),
                patches_dirs: Some(default_dexed_cartridge_dirs()),
                patch_roles: PatchRoleFilters::unfiltered(),
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
                // 用途別カテゴリの実データ（`Pad` / `Bass` / `Arpeggio` …）は TUI 側にしか
                // 置けない。ここへ書くと play server → TUI の逆向き依存が復活する
                // （`docs/adr/0007-patch-string-decides-the-plugin.md` / `0010`）。
                patch_roles: PatchRoleFilters::default(),
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
    } else if matches(VAPORIZER2_PLUGIN_ID, "vaporizer") {
        PatchForm::Vvp
    } else {
        PatchForm::StateFile
    }
}

/// 組み込みプロファイルと config の `[plugins.*]` を合わせた一覧。
///
/// 同名なら config 側の「書かれている項目」が組み込みを上書きする
/// （[`resolve_active_plugin_profile`] と同じ規則）。
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

fn unknown_active_plugin_error(
    name: &str,
    from_config: &BTreeMap<String, PluginProfile>,
) -> anyhow::Error {
    let builtin = builtin_plugin_profiles()
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let configured = if from_config.is_empty() {
        "(config に [plugins.*] は書かれていません)".to_string()
    } else {
        from_config
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };
    anyhow::anyhow!(
        "active_plugin = '{name}' に対応するプロファイルがありません。\
         組み込みで使える名前: {builtin} / config で定義済みの名前: {configured}"
    )
}

/// `active_plugin` が指すプロファイルを解決する。
///
/// 呼び出し側は戻り値の `plugin_path` / `plugin_id` / `patches_dirs` を自分の config の
/// トップレベルフィールドへ焼き込む。これを config のロード時に済ませておくことで、
/// `cfg.plugin_path` / `cfg.patches_dirs` の読み手はプロファイルの存在を知らずに済む。
///
/// - `active_plugin` が未指定なら `Ok(None)`（既存 config の完全な後方互換）。
/// - 名前は [`builtin_plugin_profiles`] と config の `[plugins.*]` の両方から探す。
///   同名なら config 側の「書かれている項目」が組み込みを上書きする。
/// - どちらにも無ければ設定エラー。使える名前を両方とも並べて示す。
/// - トップレベルの `plugin_path`（`top_level_plugin_path`）と併記されていてもエラーにせず、
///   プロファイルを優先する。移行の途中で必ず引っかかるような conflict error にはしない。
pub fn resolve_active_plugin_profile(
    active_plugin: Option<&str>,
    from_config: &BTreeMap<String, PluginProfile>,
    top_level_plugin_path: &str,
) -> anyhow::Result<Option<PluginProfile>> {
    let Some(name) = active_plugin.map(str::trim) else {
        return Ok(None);
    };
    if name.is_empty() {
        anyhow::bail!("active_plugin に空の名前は指定できません");
    }
    let configured = lookup(from_config, name);
    let builtin = lookup(&builtin_plugin_profiles(), name);
    let profile = match (builtin, configured) {
        (Some(builtin), Some(configured)) => builtin.overridden_by(configured),
        (Some(builtin), None) => builtin,
        (None, Some(configured)) => configured,
        (None, None) => return Err(unknown_active_plugin_error(name, from_config)),
    };
    if profile.plugin_path.trim().is_empty() {
        anyhow::bail!(
            "active_plugin = '{name}' の plugin_path が空です。\
             [plugins.{name}] に plugin_path を書いてください"
        );
    }
    if !top_level_plugin_path.trim().is_empty() {
        eprintln!(
            "config.toml: active_plugin = '{name}' のプロファイルを使います\
             （トップレベルの plugin_path / patches_dirs は無視します）"
        );
    }
    Ok(Some(profile))
}

#[cfg(test)]
mod tests;
