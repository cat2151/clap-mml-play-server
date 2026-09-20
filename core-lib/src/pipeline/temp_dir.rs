//! stateless render の中間ファイルを置く一時ディレクトリ。drop で消える。

use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) struct RenderTempDir {
    path: std::path::PathBuf,
}

impl RenderTempDir {
    pub(super) fn create() -> Result<Self> {
        let base = std::env::temp_dir();
        let process_id = std::process::id();
        for _ in 0..100 {
            let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("cmrt_stateless_render_{process_id}_{counter}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "一時ディレクトリの作成に失敗 ({}): {}",
                        path.display(),
                        e
                    ));
                }
            }
        }
        anyhow::bail!("一時ディレクトリ名を確保できませんでした")
    }

    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for RenderTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
