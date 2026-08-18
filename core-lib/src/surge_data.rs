//! Surge XT の `init()` を速くするための最小データディレクトリ。
//!
//! Surge XT の `init()` は `SURGE_DATA_HOME`（既定 `%ProgramData%\Surge XT`）配下の
//! `patches_factory` / `patches_3rdparty`（既定で 3,000 件超）を **インスタンスごとに** 走査する。
//! そのリストは Surge 自身のパッチブラウザ用で、cmrt は GUI を出さないので誰も読まない
//! （`.fxp` は [`crate::render`] が自分で `fs::read` して `clap_plugin_state.load` へ流している）。
//! CLAP 側にも Surge 側にも「走査を止めさせる」ホスト向けの口は無いので、
//! ホストに残るレバーは「データパスに何を置くか」だけになる。
//!
//! そこで patches を初期パッチ（`patches_factory/Templates`）だけに絞ったデータディレクトリを作り、
//! `SURGE_DATA_HOME` をそこへ向ける。実測で 16 インスタンス生成が 9.6 秒 → 2.5 秒になった。
//! 実体はジャンクション（Windows では管理者権限不要）で共有するので、
//! ディスク使用量はほぼ増えず、元のデータディレクトリは読むだけで改変しない。
//!
//! **`patches_factory/Templates` を空にしてはいけない。** Surge は初期パッチ（Init Saw 等）を
//! ここから読む。空にすると初期状態が変わり、その後 `.fxp` を読み込んでも出音が変わる
//! （実測で全パッチ RMS +26%、約 +2dB）。`.fxp` は全パラメータを書いているわけではなく、
//! 書かれていない分が初期状態から残るため。

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context as _, Result};

use crate::pipeline::ensure_cmrt_dir;

/// 最小データディレクトリの名前。`config_local_dir()/clap-mml-render-tui/` の下に作る。
const MINIMAL_DIR_NAME: &str = "surge-data-min";
/// 「このディレクトリは cmrt が作った最小構成で、元データはここ」を記録するファイル。
/// 存在そのものが最小データディレクトリの目印にもなる。
const SOURCE_MARKER_NAME: &str = "cmrt-source-data-home.txt";
const PATCHES_FACTORY: &str = "patches_factory";
const PATCHES_3RDPARTY: &str = "patches_3rdparty";
const TEMPLATES: &str = "Templates";
const WAVETABLES: &str = "wavetables";
const SURGE_DATA_HOME_ENV: &str = "SURGE_DATA_HOME";
/// `0` / `false` を入れると最小データディレクトリを使わず Surge の既定動作に戻す。
const DISABLE_ENV: &str = "CMRT_SURGE_MINIMAL_DATA_HOME";

/// [`apply_minimal_surge_data_home`] の結果。呼び出し側のログ用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimalSurgeDataHome {
    /// 実体を共有している元のデータディレクトリ。読むだけで改変しない。
    pub source: PathBuf,
    /// `SURGE_DATA_HOME` へ設定したパス。
    pub path: PathBuf,
    /// 今回ツリーを作り直したか。作り直し不要だった 2 回目以降は `false`。
    pub rebuilt: bool,
}

/// Surge XT の CLAP plugin ID。組み込みプロファイルが config へ入れる値と同じ。
pub const SURGE_XT_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";

/// このプラグインが Surge XT か。
///
/// [`apply_minimal_surge_data_home`] は Surge XT 専用の最適化なので、他のプラグイン
/// （Dexed 等）で起動したときに Surge のデータディレクトリを探して警告を出さないよう、
/// 呼ぶ前にこれで振り分ける。
///
/// `plugin_id` は config の値（`plugins.*.plugin_id`）。あればそれだけで確実に決まる。
/// 無いのは `active_plugin` を書いていない既定の config で、そのときだけ
/// [`plugin_path_looks_like_surge`] のファイル名推測へ落とす。
///
/// `std::env::set_var` はスレッド生成前に呼ぶ必要があるため、CLAP をロードして
/// descriptor を読んだ後の「本物の ID」はここでは使えない。材料は config だけ。
pub fn plugin_is_surge(plugin_id: Option<&str>, plugin_path: &str) -> bool {
    match plugin_id {
        Some(plugin_id) => plugin_id == SURGE_XT_PLUGIN_ID,
        None => plugin_path_looks_like_surge(plugin_path),
    }
}

/// この plugin path が Surge XT を指していそうか。
///
/// 判定はファイル名に `surge` を含むか（ASCII 大小無視）という粗いもの。config が
/// `plugin_id` を持たないときの最後の手段で、[`plugin_is_surge`] からだけ呼ぶ。
fn plugin_path_looks_like_surge(plugin_path: &str) -> bool {
    Path::new(plugin_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().contains("surge"))
}

/// 最小データディレクトリを用意して `SURGE_DATA_HOME` へ設定する。
///
/// `Err` は「最適化を諦めて Surge の既定動作に戻した」という意味しか持たない。
/// 呼び出し側はログに残して処理を続けること。環境変数を設定しないだけなので動作は壊れない。
///
/// # 呼ぶ場所
/// `std::env::set_var` は他スレッドが走っていると unsound なので、
/// **[`crate::load_entry`] より前、かつスレッドを spawn する前**（`main()` の早い段階）で呼ぶこと。
pub fn apply_minimal_surge_data_home() -> Result<MinimalSurgeDataHome> {
    if disabled_by_env() {
        anyhow::bail!("{DISABLE_ENV} により無効化されています");
    }
    if let Some(applied) = already_applied()? {
        return Ok(applied);
    }
    let source = resolve_source_data_home()?;
    let path = ensure_cmrt_dir()?.join(MINIMAL_DIR_NAME);
    let rebuilt = sync_minimal_tree(&source, &path)?;
    ensure_templates_not_empty(&path)?;
    std::env::set_var(SURGE_DATA_HOME_ENV, &path);
    Ok(MinimalSurgeDataHome {
        source,
        path,
        rebuilt,
    })
}

fn disabled_by_env() -> bool {
    match std::env::var(DISABLE_ENV) {
        Ok(value) => matches!(value.trim(), "0" | "false" | "no"),
        Err(_) => false,
    }
}

/// 既に最小データディレクトリを指している（親プロセスから環境変数を継承した等）なら、
/// 何もせずその情報を返す。最小ディレクトリを元データとして再帰的に縮める事故を防ぐ。
fn already_applied() -> Result<Option<MinimalSurgeDataHome>> {
    let Some(current) = std::env::var_os(SURGE_DATA_HOME_ENV) else {
        return Ok(None);
    };
    let path = PathBuf::from(current);
    let marker = path.join(SOURCE_MARKER_NAME);
    if !marker.is_file() {
        return Ok(None);
    }
    let source =
        fs::read_to_string(&marker).with_context(|| format!("{} が読めない", marker.display()))?;
    Ok(Some(MinimalSurgeDataHome {
        source: PathBuf::from(source.trim()),
        path,
        rebuilt: false,
    }))
}

/// 実体を共有する元データディレクトリを決める。
/// 利用者が `SURGE_DATA_HOME` を設定していればその位置を尊重する。
fn resolve_source_data_home() -> Result<PathBuf> {
    let candidate = match std::env::var_os(SURGE_DATA_HOME_ENV) {
        Some(value) => PathBuf::from(value),
        None => default_surge_data_home().ok_or_else(|| {
            anyhow!("Surge XT データディレクトリの既定位置が分からないプラットフォームです")
        })?,
    };
    if !looks_like_surge_data_home(&candidate) {
        anyhow::bail!(
            "Surge XT のデータディレクトリに見えません: {}",
            candidate.display()
        );
    }
    Ok(candidate)
}

#[cfg(windows)]
fn default_surge_data_home() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|dir| PathBuf::from(dir).join("Surge XT"))
}

#[cfg(not(windows))]
fn default_surge_data_home() -> Option<PathBuf> {
    // Surge XT の配置がディストリビューション依存なので推測しない。
    // 最適化を諦めるだけで、既定動作のままレンダリングはできる。
    None
}

fn looks_like_surge_data_home(path: &Path) -> bool {
    path.join(PATCHES_FACTORY).join(TEMPLATES).is_dir() && path.join(WAVETABLES).is_dir()
}

/// 最小ツリーが元データと噛み合っていれば何もしない。噛み合っていなければ作り直す。
/// 戻り値は「作り直したか」。
fn sync_minimal_tree(source: &Path, minimal: &Path) -> Result<bool> {
    let shared = shared_entry_names(source)?;
    if tree_is_current(minimal, source, &shared)? {
        return Ok(false);
    }
    rebuild_tree(minimal, source, &shared)?;
    Ok(true)
}

/// 元データからそのまま共有する最上位エントリ名。patches 系だけを除く。
/// Surge の更新でディレクトリが増減しても追従できるよう、固定リストにはしない。
fn shared_entry_names(source: &Path) -> Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(source)
        .with_context(|| format!("データディレクトリが読めない: {}", source.display()))?
    {
        let name = entry?.file_name();
        if name == PATCHES_FACTORY || name == PATCHES_3RDPARTY {
            continue;
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn tree_is_current(minimal: &Path, source: &Path, shared: &[OsString]) -> Result<bool> {
    let Ok(recorded) = fs::read_to_string(minimal.join(SOURCE_MARKER_NAME)) else {
        return Ok(false);
    };
    if Path::new(recorded.trim()) != source {
        return Ok(false);
    }
    if !minimal.join(PATCHES_3RDPARTY).is_dir()
        || !minimal.join(PATCHES_FACTORY).join(TEMPLATES).is_dir()
    {
        return Ok(false);
    }
    // `exists()` はリンクを辿るので、実体を失ったジャンクションもここで落ちる。
    if shared.iter().any(|name| !minimal.join(name).exists()) {
        return Ok(false);
    }
    Ok(entry_names(minimal)? == expected_entry_names(shared))
}

fn entry_names(dir: &Path) -> Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("{} が読めない", dir.display()))? {
        names.push(entry?.file_name());
    }
    names.sort();
    Ok(names)
}

fn expected_entry_names(shared: &[OsString]) -> Vec<OsString> {
    let mut names = shared.to_vec();
    names.push(OsString::from(PATCHES_FACTORY));
    names.push(OsString::from(PATCHES_3RDPARTY));
    names.push(OsString::from(SOURCE_MARKER_NAME));
    names.sort();
    names
}

fn rebuild_tree(minimal: &Path, source: &Path, shared: &[OsString]) -> Result<()> {
    let templates_source = source.join(PATCHES_FACTORY).join(TEMPLATES);
    if !templates_source.is_dir() {
        anyhow::bail!(
            "初期パッチのディレクトリが無い: {}",
            templates_source.display()
        );
    }
    fs::create_dir_all(minimal).with_context(|| format!("{} の作成に失敗", minimal.display()))?;
    clear_dir(minimal)?;

    for name in shared {
        let target = source.join(name);
        let link = minimal.join(name);
        if target.is_dir() {
            link_dir(&link, &target)?;
        } else {
            fs::copy(&target, &link)
                .with_context(|| format!("{} のコピーに失敗", target.display()))?;
        }
    }

    let factory = minimal.join(PATCHES_FACTORY);
    fs::create_dir(&factory).with_context(|| format!("{} の作成に失敗", factory.display()))?;
    link_dir(&factory.join(TEMPLATES), &templates_source)?;
    let third_party = minimal.join(PATCHES_3RDPARTY);
    fs::create_dir(&third_party)
        .with_context(|| format!("{} の作成に失敗", third_party.display()))?;

    let marker = minimal.join(SOURCE_MARKER_NAME);
    fs::write(&marker, format!("{}\n", source.display()))
        .with_context(|| format!("{} の書き込みに失敗", marker.display()))?;
    Ok(())
}

fn clear_dir(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("{} が読めない", dir.display()))? {
        remove_entry(&entry?.path())?;
    }
    Ok(())
}

/// リンク（ジャンクション / シンボリックリンク）は実体を辿らずリンクだけ消す。
/// 元データを巻き込んで削除しないための要点なので、`fs::remove_dir_all` は使わない。
fn remove_entry(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("{} の情報が取れない", path.display()))
        }
    };
    let result = if is_link(&metadata) {
        remove_link(path, &metadata)
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path).with_context(|| format!("{} が読めない", path.display()))?
        {
            remove_entry(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    result.with_context(|| format!("{} の削除に失敗", path.display()))
}

#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    // ジャンクションは `FileType::is_symlink()` でも true になるが、
    // 「reparse point なら絶対に辿らない」方が安全側なので属性で判定する。
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn remove_link(path: &Path, metadata: &fs::Metadata) -> std::io::Result<()> {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
    if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(not(windows))]
fn remove_link(path: &Path, _metadata: &fs::Metadata) -> std::io::Result<()> {
    fs::remove_file(path)
}

/// `link` から `target` への、実体を共有するディレクトリ別名を作る。
#[cfg(windows)]
fn link_dir(link: &Path, target: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // ジャンクションなら管理者権限も開発者モードも不要。
    // `std::os::windows::fs::symlink_dir` は権限が要るので使えない。
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .with_context(|| format!("mklink の起動に失敗: {}", link.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "ジャンクションを作れない ({} -> {}): {}",
            link.display(),
            target.display(),
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn link_dir(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
        format!(
            "シンボリックリンクを作れない ({} -> {})",
            link.display(),
            target.display()
        )
    })
}

/// 初期パッチが 1 つも見えない構成は出音を変えてしまうので、環境変数を設定せず諦める。
fn ensure_templates_not_empty(minimal: &Path) -> Result<()> {
    let templates = minimal.join(PATCHES_FACTORY).join(TEMPLATES);
    let count = fs::read_dir(&templates)
        .with_context(|| format!("{} が読めない", templates.display()))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("fxp"))
        })
        .count();
    if count == 0 {
        anyhow::bail!(
            "初期パッチ (.fxp) が 1 つも無い: {}（空にすると出音が変わるため既定動作に戻す）",
            templates.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
