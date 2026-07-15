use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_output(manifest_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|stdout| stdout.trim().to_owned())
        .filter(|stdout| !stdout.is_empty())
}

fn resolved_git_path(manifest_dir: &Path, git_path: &str) -> PathBuf {
    let path = PathBuf::from(git_path);
    if path.is_absolute() {
        path
    } else {
        manifest_dir.join(path)
    }
}

fn build_commit_hash(manifest_dir: &Path) -> String {
    git_output(manifest_dir, &["rev-parse", "HEAD"])
        .or_else(|| env::var("GITHUB_SHA").ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));

    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    if let Some(head_path) = git_output(&manifest_dir, &["rev-parse", "--git-path", "HEAD"]) {
        println!(
            "cargo:rerun-if-changed={}",
            resolved_git_path(&manifest_dir, &head_path).display()
        );
    }

    if let Some(reference) = git_output(&manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        if let Some(reference_path) =
            git_output(&manifest_dir, &["rev-parse", "--git-path", &reference])
        {
            println!(
                "cargo:rerun-if-changed={}",
                resolved_git_path(&manifest_dir, &reference_path).display()
            );
        }
    }

    println!(
        "cargo:rustc-env=BUILD_COMMIT_HASH={}",
        build_commit_hash(&manifest_dir)
    );
}
