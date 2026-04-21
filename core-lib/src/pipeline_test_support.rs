/// `CMRT_BASE_DIR` 環境変数を変更するテストを直列化するためのグローバル Mutex。
///
/// 複数のテストが並行して同じ環境変数を変更しないよう、環境変数を操作するすべてのテストは
/// `EnvVarGuard::set()` を通じてこのロックを取得してから処理を行う。
/// `ensure_cmrt_dir()` を使用するが環境変数を変更しないテストは `env_lock()` で直列化する。
static ENV_MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

/// `CMRT_BASE_DIR` を変更しないが `ensure_cmrt_dir()` を使用するテスト向けのロック取得ヘルパー。
///
/// 環境変数を変更するテストと同じ Mutex を取得することで、CMRT_BASE_DIR が一時ディレクトリを
/// 指している最中に `ensure_cmrt_dir()` を呼び出さないことを保証する。
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// `CMRT_BASE_DIR` 環境変数の設定とロックの取得を一体化した RAII ガード。
///
/// - 構築時にグローバル Mutex を取得し、以前の値を退避してから環境変数を設定する。
/// - `Drop` 時に元の値を復元する（テストがパニックで終了した場合も含む）。
/// - Mutex ガードはこの型の生存期間中保持されるため、複数テスト間の並行実行が防止される。
pub(crate) struct EnvVarGuard {
    key: &'static str,
    original: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    /// 環境変数 `key` を `value` に設定し、Mutex ロックと元の値を保持するガードを返す。
    pub(crate) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_MUTEX
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let original = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key,
            original,
            _lock: lock,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
