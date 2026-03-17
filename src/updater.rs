//! 自動アップデート機能。
//! 起動時にGitHubのmainブランチのhashをチェックし、
//! ローカルのhashと異なる場合は問答無用でアップデートを実行する。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::Result;

const REPO_OWNER: &str = "cat2151";
const REPO_NAME: &str = "clap-mml-render-tui";

/// ビルド時に埋め込まれたgit commit hash
const LOCAL_HASH: &str = env!("GIT_COMMIT_HASH");

/// ローカルhashが有効なSHA-1の40文字16進数文字列かを確認する
fn is_valid_sha1(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// ETagのキャッシュファイルパスを返す
fn etag_cache_file() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|d| d.join("clap-mml-render-tui").join("github_etag"))
}

/// キャッシュされたETagを読み込む
fn load_cached_etag() -> Option<String> {
    let path = etag_cache_file()?;
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// ETagをキャッシュファイルに保存する
fn save_etag(etag: &str) {
    let Some(path) = etag_cache_file() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, etag);
}

/// `gh auth token` コマンドからGitHubアクセストークンを取得する（利用可能な場合）
fn get_gh_token() -> Option<String> {
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// リモートのmainブランチの最新commit hashをGitHub APIで取得する。
/// ETagが一致する場合（変更なし）は `Ok(None)` を返す。
fn fetch_remote_hash() -> Result<Option<String>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/commits/main",
        REPO_OWNER, REPO_NAME
    );

    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(10))
        .timeout_write(std::time::Duration::from_secs(10))
        .build();

    let mut req = agent
        .get(&url)
        .set("User-Agent", "clap-mml-render-tui-updater")
        .set("Accept", "application/vnd.github.v3+json");

    // gh コマンドから認証トークンを取得して使用する（利用可能な場合）
    if let Some(token) = get_gh_token() {
        req = req.set("Authorization", &format!("Bearer {}", token));
    }

    // キャッシュされたETagがあれば条件付きリクエストを送る
    let cached_etag = load_cached_etag();
    if let Some(ref etag) = cached_etag {
        req = req.set("If-None-Match", etag);
    }

    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(304, _)) => {
            // 304 Not Modified — リモートのhashは変わっていない
            return Ok(None);
        }
        Err(_) => return Ok(None), // ネットワークエラーはサイレントに無視
    };

    // 新しいETagを保存する
    if let Some(etag) = resp.header("ETag") {
        save_etag(etag);
    }

    let body: serde_json::Value = resp.into_json()?;
    let sha = body["sha"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("SHA field not found in GitHub API response"))?;

    Ok(Some(sha))
}

/// バックグラウンドでアップデートチェックを実行する。
/// 更新が必要な場合は `update_available` を true にセットする。
pub fn spawn_update_check(update_available: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        if let Err(_e) = check_for_update(update_available) {
            // TUI動作中のためeprintlnは使わない（表示崩れ防止）
        }
    });
}

fn check_for_update(update_available: Arc<AtomicBool>) -> Result<()> {
    // デバッグビルド時は自動アップデートをスキップ（開発中の誤更新を防止）
    if cfg!(debug_assertions) {
        return Ok(());
    }

    // ローカルhashが有効なSHA-1でなければスキップ（不明なビルド環境）
    let local = LOCAL_HASH.trim();
    if !is_valid_sha1(local) {
        return Ok(());
    }

    // リモートhashを取得（ETagで変更なしの場合は None）
    let remote_hash = match fetch_remote_hash()? {
        Some(h) => h,
        None => return Ok(()), // 304 Not Modified またはネットワークエラー
    };

    // リモートhashがlocal hashと一致していれば何もしない
    if remote_hash == local {
        return Ok(());
    }

    // アップデートが利用可能: フラグをセット
    update_available.store(true, Ordering::Relaxed);

    Ok(())
}

/// フォアグラウンドでアップデートを実行する。
/// TUIを終了してから呼び出すこと。
pub fn run_foreground_update() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        println!("アップデートをバッチファイルで開始します...");
        spawn_updater_process()
            .map_err(|e| anyhow::anyhow!("バッチファイルアップデーターの起動に失敗しました: {}", e))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        println!("アップデートを開始します...");
        println!(
            "cargo install --force --git https://github.com/{}/{}",
            REPO_OWNER, REPO_NAME
        );

        let status = std::process::Command::new("cargo")
            .args([
                "install",
                "--force",
                "--git",
                &format!("https://github.com/{}/{}", REPO_OWNER, REPO_NAME),
            ])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if status.success() {
            println!("アップデート成功！再起動します...");
            match std::process::Command::new("cmrt").spawn() {
                Ok(_) => {
                    // 新しいプロセスを起動したら現在のプロセスを終了する（二重起動を防ぐ）
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("cmrtの再起動に失敗しました: {}。手動で再起動してください。", e);
                }
            }
        } else {
            eprintln!("アップデートに失敗しました。");
        }

        Ok(())
    }
}

/// Windowsでのアップデートを行うバッチファイルをspawnする。
#[cfg(target_os = "windows")]
fn spawn_updater_process() -> Result<()> {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let script_path = std::env::temp_dir().join(format!("cmrt_updater_{}.bat", suffix));
    let script = format!(
        "@echo off\r\ntimeout /t 3 /nobreak >nul\r\ncargo install --force --git https://github.com/{}/{}\r\ncmrt\r\n(goto) 2>nul & del \"%~f0\"\r\n",
        REPO_OWNER, REPO_NAME
    );
    std::fs::write(&script_path, &script)?;
    let script_str = script_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Updater script path contains invalid UTF-8"))?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "cmrt updater", script_str])
        .spawn()?;
    Ok(())
}
