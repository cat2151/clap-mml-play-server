//! Six Sines の factory 音色（`.sxsnp`）を GitHub から取得してディスクに置く。
//!
//! factory 音色は `.clap` に埋め込まれていてディスク上に無く、plugin は preset-discovery も
//! 持たない。そこで descriptor の version 末尾にある commit の短縮 hash を手がかりに、
//! 同じ commit の `resources/factory_patches/` を GitHub から取る。
//!
//! 置き場は [`cmrt_server_config::six_sines_factory_dir`]。取得済みの commit は置き場の外の
//! 兄弟ファイル（`six-sines-factory.commit`）に記録し、同じ commit なら通信しない。
//! 途中で失敗したときは旧置き場と記録を残す。

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use cmrt_server_config::SIX_SINES_PLUGIN_ID;

const REPO: &str = "baconpaul/six-sines";
const FACTORY_ROOT: &str = "resources/factory_patches";
const PATCH_EXTENSION: &str = ".sxsnp";
/// GitHub API は `User-Agent` が無いリクエストを拒む。
const USER_AGENT: &str = "clap-mml-render-tui";
const COMMIT_RECORD_SUFFIX: &str = ".commit";
const TMP_SUFFIX: &str = ".tmp";

/// HTTP の取得口。テストで偽物に差し替えるための境界。
pub trait FactoryFetcher {
    fn get_text(&mut self, url: &str) -> Result<String>;
    fn get_bytes(&mut self, url: &str) -> Result<Vec<u8>>;
}

/// `ureq` で実際に通信する取得口。
pub struct HttpFetcher {
    agent: ureq::Agent,
}

impl Default for HttpFetcher {
    fn default() -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(20)))
                .timeout_recv_body(Some(Duration::from_secs(20)))
                .build(),
        );
        Self { agent }
    }
}

impl HttpFetcher {
    fn call(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
        self.agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("取得に失敗: {url}"))
    }
}

impl FactoryFetcher for HttpFetcher {
    fn get_text(&mut self, url: &str) -> Result<String> {
        self.call(url)?
            .body_mut()
            .read_to_string()
            .with_context(|| format!("本文の読み取りに失敗: {url}"))
    }

    fn get_bytes(&mut self, url: &str) -> Result<Vec<u8>> {
        self.call(url)?
            .body_mut()
            .read_to_vec()
            .with_context(|| format!("本文の読み取りに失敗: {url}"))
    }
}

/// 取得の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactorySync {
    /// 記録済みの commit と同じだったので通信しなかった。
    Unchanged { commit: String },
    /// 置き場を `commit` の内容で置き換えた。
    Downloaded { commit: String, patches: usize },
}

impl std::fmt::Display for FactorySync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unchanged { commit } => write!(f, "取得済み (commit {commit})"),
            Self::Downloaded { commit, patches } => {
                write!(f, "{patches} 件を取得 (commit {commit})")
            }
        }
    }
}

/// `plugin_path` の Six Sines に合う factory 音色を既定の置き場へ揃える。
pub fn sync_six_sines_factory(plugin_path: &str) -> Result<FactorySync> {
    let dest =
        cmrt_server_config::six_sines_factory_dir().context("設定ディレクトリが取得できない")?;
    sync_six_sines_factory_with(plugin_path, &dest, &mut HttpFetcher::default())
}

/// [`sync_six_sines_factory`] の置き場と取得口を差し替えられる版。
pub fn sync_six_sines_factory_with(
    plugin_path: &str,
    dest: &Path,
    fetcher: &mut impl FactoryFetcher,
) -> Result<FactorySync> {
    let version = six_sines_version(plugin_path)?;
    let commit = commit_from_version(&version)?;
    sync_factory_commit(&commit, dest, fetcher)
}

fn six_sines_version(plugin_path: &str) -> Result<String> {
    let entry = crate::load_entry(plugin_path)?;
    let descriptor = crate::select_descriptor(&entry, Some(SIX_SINES_PLUGIN_ID))
        .with_context(|| format!("plugin_path={plugin_path}"))?;
    Ok(descriptor.version)
}

/// descriptor version（例 `1.2.0.18ecb36`）の最後の `.` の後ろを commit hash として返す。
pub fn commit_from_version(version: &str) -> Result<String> {
    let tail = version.rsplit('.').next().unwrap_or_default().trim();
    let is_hash = (7..=40).contains(&tail.len())
        && tail
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    anyhow::ensure!(
        is_hash,
        "Six Sines の version '{version}' の末尾に commit hash が無い"
    );
    Ok(tail.to_string())
}

/// `commit` の factory 音色を `dest` へ揃える。記録済みの commit と同じなら何もしない。
pub fn sync_factory_commit(
    commit: &str,
    dest: &Path,
    fetcher: &mut impl FactoryFetcher,
) -> Result<FactorySync> {
    let record = sibling_path(dest, COMMIT_RECORD_SUFFIX)?;
    let recorded = std::fs::read_to_string(&record).ok();
    if recorded.as_deref().map(str::trim) == Some(commit) && dest.is_dir() {
        return Ok(FactorySync::Unchanged {
            commit: commit.to_string(),
        });
    }

    let tmp = sibling_path(dest, TMP_SUFFIX)?;
    let result = download_into(commit, &tmp, fetcher);
    let patches = match result {
        Ok(patches) => patches,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(error);
        }
    };
    if dest.exists() {
        std::fs::remove_dir_all(dest)
            .with_context(|| format!("旧置き場を消せない: {}", dest.display()))?;
    }
    std::fs::rename(&tmp, dest).with_context(|| {
        format!(
            "置き場を差し替えられない: {} -> {}",
            tmp.display(),
            dest.display()
        )
    })?;
    std::fs::write(&record, commit)
        .with_context(|| format!("commit の記録を書けない: {}", record.display()))?;
    Ok(FactorySync::Downloaded {
        commit: commit.to_string(),
        patches,
    })
}

/// 置き場と同じ親に置く兄弟ファイル（`<置き場名><suffix>`）。
fn sibling_path(dest: &Path, suffix: &str) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .with_context(|| format!("置き場の名前が無い: {}", dest.display()))?;
    let mut sibling = name.to_os_string();
    sibling.push(suffix);
    Ok(dest.with_file_name(sibling))
}

fn download_into(commit: &str, tmp: &Path, fetcher: &mut impl FactoryFetcher) -> Result<usize> {
    let tree_url = format!(
        "https://api.github.com/repos/{REPO}/git/trees/{commit}:{FACTORY_ROOT}?recursive=1"
    );
    let entries = parse_tree(&fetcher.get_text(&tree_url)?)?;
    anyhow::ensure!(
        !entries.is_empty(),
        "commit {commit} の {FACTORY_ROOT} に {PATCH_EXTENSION} が無い"
    );
    if tmp.exists() {
        std::fs::remove_dir_all(tmp)
            .with_context(|| format!("前回の一時置き場を消せない: {}", tmp.display()))?;
    }
    for entry in &entries {
        let url = format!(
            "https://raw.githubusercontent.com/{REPO}/{commit}/{FACTORY_ROOT}/{}",
            encode_path(&entry.path)
        );
        let bytes = fetcher.get_bytes(&url)?;
        anyhow::ensure!(
            bytes.len() as u64 == entry.size,
            "{} の大きさが tree と違う (tree {} / 取得 {})",
            entry.path,
            entry.size,
            bytes.len()
        );
        let file = tmp.join(&entry.path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("ディレクトリを作れない: {}", parent.display()))?;
        }
        std::fs::write(&file, bytes)
            .with_context(|| format!("書き込めない: {}", file.display()))?;
    }
    Ok(entries.len())
}

#[derive(Debug, PartialEq, Eq)]
struct TreeBlob {
    path: String,
    size: u64,
}

/// tree API の JSON から `.sxsnp` の blob を取り出す。`truncated` の tree は全件が揃わないので拒む。
fn parse_tree(json: &str) -> Result<Vec<TreeBlob>> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("tree API の応答が JSON でない")?;
    anyhow::ensure!(
        value["truncated"] == serde_json::Value::Bool(false),
        "tree API の応答が truncated（全件が揃わない）"
    );
    let items = value["tree"]
        .as_array()
        .context("tree API の応答に tree 配列が無い")?;
    let mut blobs = Vec::new();
    for item in items {
        let path = item["path"].as_str().unwrap_or_default();
        if item["type"] != "blob" || !path.ends_with(PATCH_EXTENSION) {
            continue;
        }
        anyhow::ensure!(
            path.split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
            "tree のパスが置き場の外を指す: {path}"
        );
        let size = item["size"]
            .as_u64()
            .with_context(|| format!("{path} に size が無い"))?;
        blobs.push(TreeBlob {
            path: path.to_string(),
            size,
        });
    }
    Ok(blobs)
}

/// `/` 区切りのパスを、区切りは残して各要素を percent-encode する。
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests;
