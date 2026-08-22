//! 1 プロセスへ同時に載せうるプラグインの種別と、その材料。
//!
//! 「どのプラグインをどこから読むか」「そのプラグインの音色置き場はどこか」は
//! `cmrt_server_config` のプロファイルが持つ知識。ここはそれを、インスタンス生成に
//! そのまま使える形（`CoreConfig` 1 件）へ落とすだけ。
//!
//! # なぜ core-lib にあるか
//! 使うのは realtime play server（論理スロットへ載せる物理インスタンス）だけではない。
//! オフラインの render server も、受け取った MML の音色でどのプラグインへ渡すかを
//! 決める必要がある（`docs/adr/0007-patch-string-decides-the-plugin.md`）。**判別規則が 2 か所に
//! 分かれると、片方だけ直したときに「操作は成功したが前の音のまま」という静かな
//! 間違いになる**ので、種別の一覧と patch → 種別の引き当てはここへ 1 本化する。

use std::path::Path;

use crate::{is_cartridge_patch_path, is_vvp_patch_path, CoreConfig};
use cmrt_server_config::{patch_form_of, PatchForm, ServerConfig};

/// 1 プロセスへ載せうるプラグイン 1 種別。
#[derive(Clone, Debug)]
pub struct PluginKind {
    /// プロファイル名。ログとエラー文にしか使わない。
    pub name: String,
    pub plugin_path: String,
    /// この種別の音色が patch 文字列としてどう書かれるか。
    pub patch_form: PatchForm,
    /// この種別のインスタンスを作るときの設定。`plugin_id` と `patches_dir` が種別ごとに違う。
    pub core_cfg: CoreConfig,
}

/// 使えるプラグインの種別一覧。先頭が既定プラグイン（音色無指定の行が鳴るもの）。
///
/// 既定は `active_plugin` の解決結果、つまり `cfg` のトップレベルへ焼き込まれた値を
/// そのまま使う。残りは同じ config から引ける他のプロファイルのうち、**本体が実在する**もの。
/// 実在しないものを候補に残すと、差し替えのたびにロード失敗で初めて気付くことになる。
pub fn plugin_kinds(cfg: &ServerConfig, core_cfg: &CoreConfig) -> Vec<PluginKind> {
    let default_kind = PluginKind {
        name: cfg
            .active_plugin
            .clone()
            .unwrap_or_else(|| cmrt_server_config::plugin_file_stem(&cfg.plugin_path)),
        plugin_path: cfg.plugin_path.clone(),
        patch_form: patch_form_of(cfg.plugin_id.as_deref(), &cfg.plugin_path),
        core_cfg: core_cfg.clone(),
    };
    let mut kinds = vec![default_kind];
    for (name, profile) in cfg.installed_plugin_profiles() {
        if same_plugin(&profile.plugin_path, &kinds[0].plugin_path) {
            continue;
        }
        kinds.push(PluginKind {
            name,
            plugin_path: profile.plugin_path.clone(),
            patch_form: patch_form_of(profile.plugin_id.as_deref(), &profile.plugin_path),
            core_cfg: CoreConfig {
                plugin_id: profile.plugin_id.clone(),
                // 起動時の音色（config の `patch_path`）は既定プラグイン向けの指定なので、
                // 他の種別へ持ち込まない。持ち込むと形の違う音色を読もうとして失敗する。
                patch_path: None,
                patches_dir: cmrt_server_config::patch_root_dir(profile.patches_dirs.as_deref()),
                ..core_cfg.clone()
            },
        });
    }
    kinds
}

/// patch 文字列がどのプラグインを要求しているか。`kinds` の添字を返す。
///
/// 判別の材料は文字列の形だけ（`docs/adr/0007-patch-string-decides-the-plugin.md`）。無指定は
/// 既定プラグインへ固定する。そうしておくと、無指定の行が鳴るプラグインが常に
/// 1 つに決まり、MML 文字列を鍵にしている cache が衝突しない（同 §5.3）。
///
/// 同じ形を扱う種別が複数あるときは既定プラグインを優先し、無ければ先頭のものを使う。
pub fn kind_for_patch(
    kinds: &[PluginKind],
    default_kind: usize,
    patch: Option<&str>,
) -> Result<usize, String> {
    let Some(patch) = patch else {
        return Ok(default_kind);
    };
    let form = patch_form_of_path(patch);
    if kinds[default_kind].patch_form == form {
        return Ok(default_kind);
    }
    kinds
        .iter()
        .position(|kind| kind.patch_form == form)
        .ok_or_else(|| {
            let installed = kinds
                .iter()
                .map(|kind| kind.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "この音色を読めるプラグインがインストールされていない: '{patch}'（使えるプラグイン: {installed}）"
            )
        })
}

/// patch 文字列そのものから形を読む。
///
/// [`kind_for_patch`] と [`PatchBases::base_for`] の両方が同じ規則を使う。
/// 別々に書くと、片方だけ直したときに「選ばれたプラグインと基点が食い違う」
/// という静かな間違いになる。
fn patch_form_of_path(patch: &str) -> PatchForm {
    if is_cartridge_patch_path(patch) {
        PatchForm::Cartridge
    } else if is_vvp_patch_path(patch) {
        PatchForm::Vvp
    } else {
        PatchForm::StateFile
    }
}

/// patch 文字列の形ごとの、相対パスを絶対パスへ直す基点。
///
/// 音色置き場はプラグインごとに別の場所なので、基点も形ごとに分かれる。
#[derive(Clone, Debug, Default)]
pub struct PatchBases {
    state_file: Option<String>,
    cartridge: Option<String>,
    vvp: Option<String>,
}

impl PatchBases {
    pub fn from_kinds(kinds: &[PluginKind]) -> Self {
        let mut bases = Self::default();
        for kind in kinds {
            let slot = bases.slot_mut(kind.patch_form);
            // 先頭（既定プラグイン）の基点を優先する。
            if slot.is_none() {
                slot.clone_from(&kind.core_cfg.patches_dir);
            }
        }
        bases
    }

    /// 基点を直接指定して作る。プラグイン一覧を組まずに解決だけ確かめたいとき用。
    pub fn from_bases(
        state_file: Option<&str>,
        cartridge: Option<&str>,
        vvp: Option<&str>,
    ) -> Self {
        Self {
            state_file: state_file.map(str::to_string),
            cartridge: cartridge.map(str::to_string),
            vvp: vvp.map(str::to_string),
        }
    }

    /// この patch 文字列に対応する基点。
    pub fn base_for(&self, patch: &str) -> Option<&str> {
        match patch_form_of_path(patch) {
            PatchForm::StateFile => self.state_file.as_deref(),
            PatchForm::Cartridge => self.cartridge.as_deref(),
            PatchForm::Vvp => self.vvp.as_deref(),
        }
    }

    fn slot_mut(&mut self, form: PatchForm) -> &mut Option<String> {
        match form {
            PatchForm::StateFile => &mut self.state_file,
            PatchForm::Cartridge => &mut self.cartridge,
            PatchForm::Vvp => &mut self.vvp,
        }
    }
}

/// 同じプラグイン本体を指しているか。既定プラグインとプロファイルの重複を避けるための比較。
fn same_plugin(left: &str, right: &str) -> bool {
    let normalize = |path: &str| {
        let path = Path::new(path.trim());
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests;
