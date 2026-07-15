use std::error::Error;

use anyhow::{anyhow, Result};
use cat_self_update_lib::{check_remote_commit, self_update, CheckResult};

const UPDATE_REPO_OWNER: &str = "cat2151";
const UPDATE_REPO_NAME: &str = "clap-mml-play-server";
const UPDATE_BRANCH: &str = "main";
const UPDATE_TARGETS: [&str; 2] = ["clap-mml-render-server", "clap-mml-realtime-play-server"];

type LibResult<T> = std::result::Result<T, Box<dyn Error>>;
type CheckRemoteCommitFn = fn(&str, &str, &str, &str) -> LibResult<CheckResult>;
type SelfUpdateFn = fn(&str, &str, &[&str]) -> LibResult<()>;

fn check_workspace_update_with(
    build_commit_hash: &str,
    checker: CheckRemoteCommitFn,
) -> Result<String> {
    checker(
        UPDATE_REPO_OWNER,
        UPDATE_REPO_NAME,
        UPDATE_BRANCH,
        build_commit_hash,
    )
    .map(|result| result.to_string())
    .map_err(|error| anyhow!("failed to check for workspace update: {error}"))
}

/// ビルド時に埋め込まれたcommit hashとremote mainのcommit hashを比較する。
pub fn check_workspace_update(build_commit_hash: &str) -> Result<String> {
    check_workspace_update_with(build_commit_hash, check_remote_commit)
}

fn run_workspace_update_with(updater: SelfUpdateFn) -> Result<()> {
    updater(UPDATE_REPO_OWNER, UPDATE_REPO_NAME, &UPDATE_TARGETS)
        .map_err(|error| anyhow!("failed to update workspace: {error}"))
}

/// workspace内の実行バイナリをまとめて更新する。
pub fn run_workspace_update() -> Result<()> {
    run_workspace_update_with(self_update)
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    const FAKE_ERROR_MESSAGE: &str = "network down";

    fn fake_check_success(
        owner: &str,
        repo: &str,
        branch: &str,
        embedded_hash: &str,
    ) -> LibResult<CheckResult> {
        assert_eq!(owner, "cat2151");
        assert_eq!(repo, "clap-mml-play-server");
        assert_eq!(branch, "main");
        assert_eq!(embedded_hash, "embedded123");

        Ok(CheckResult {
            embedded_hash: embedded_hash.to_owned(),
            remote_hash: embedded_hash.to_owned(),
        })
    }

    #[derive(Debug)]
    struct FakeError;

    impl fmt::Display for FakeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{FAKE_ERROR_MESSAGE}")
        }
    }

    impl Error for FakeError {}

    fn fake_check_failure(
        _owner: &str,
        _repo: &str,
        _branch: &str,
        _embedded_hash: &str,
    ) -> LibResult<CheckResult> {
        Err(Box::new(FakeError))
    }

    fn fake_update_success(owner: &str, repo: &str, targets: &[&str]) -> LibResult<()> {
        assert_eq!(owner, "cat2151");
        assert_eq!(repo, "clap-mml-play-server");
        assert_eq!(
            targets,
            ["clap-mml-render-server", "clap-mml-realtime-play-server"]
        );
        Ok(())
    }

    fn fake_update_failure(_owner: &str, _repo: &str, _targets: &[&str]) -> LibResult<()> {
        Err(Box::new(FakeError))
    }

    #[test]
    fn check_uses_workspace_repo_arguments() {
        let result = check_workspace_update_with("embedded123", fake_check_success).unwrap();

        assert_eq!(
            result,
            "embedded: embedded123\nremote: embedded123\nresult: up-to-date"
        );
    }

    #[test]
    fn check_adds_context_to_errors() {
        let error = check_workspace_update_with("embedded123", fake_check_failure).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("failed to check for workspace update: {FAKE_ERROR_MESSAGE}")
        );
    }

    #[test]
    fn update_uses_workspace_repo_and_target_arguments() {
        run_workspace_update_with(fake_update_success).unwrap();
    }

    #[test]
    fn update_adds_context_to_errors() {
        let error = run_workspace_update_with(fake_update_failure).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("failed to update workspace: {FAKE_ERROR_MESSAGE}")
        );
    }
}
