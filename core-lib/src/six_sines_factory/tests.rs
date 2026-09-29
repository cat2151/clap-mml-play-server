use std::collections::BTreeMap;

use super::*;

/// URL から応答を引く偽の取得口。`fail_on` を含む URL は失敗させる。
#[derive(Default)]
struct FakeFetcher {
    tree: String,
    files: BTreeMap<String, Vec<u8>>,
    fail_on: Option<String>,
    calls: usize,
}

impl FakeFetcher {
    fn with_files(commit: &str, files: &[(&str, &str)]) -> Self {
        let tree = files
            .iter()
            .map(|(path, body)| {
                format!(r#"{{"path":"{path}","type":"blob","size":{}}}"#, body.len())
            })
            .chain([r#"{"path":"Bass","type":"tree"}"#.to_string()])
            .chain([r#"{"path":"README.md","type":"blob","size":3}"#.to_string()])
            .collect::<Vec<_>>()
            .join(",");
        let files = files
            .iter()
            .map(|(path, body)| {
                (
                    format!(
                        "https://raw.githubusercontent.com/{REPO}/{commit}/{FACTORY_ROOT}/{}",
                        encode_path(path)
                    ),
                    body.as_bytes().to_vec(),
                )
            })
            .collect();
        Self {
            tree: format!(r#"{{"truncated":false,"tree":[{tree}]}}"#),
            files,
            ..Default::default()
        }
    }

    fn check(&mut self, url: &str) -> Result<()> {
        self.calls += 1;
        if self.fail_on.as_deref().is_some_and(|key| url.contains(key)) {
            anyhow::bail!("偽の通信失敗: {url}");
        }
        Ok(())
    }
}

impl FactoryFetcher for FakeFetcher {
    fn get_text(&mut self, url: &str) -> Result<String> {
        self.check(url)?;
        anyhow::ensure!(url.contains("/git/trees/"), "tree 以外の text 取得: {url}");
        Ok(self.tree.clone())
    }

    fn get_bytes(&mut self, url: &str) -> Result<Vec<u8>> {
        self.check(url)?;
        self.files
            .get(url)
            .cloned()
            .with_context(|| format!("偽の取得口に無い URL: {url}"))
    }
}

fn fresh_dest(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("cmrt_test_six_sines_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.join("six-sines-factory")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn record_of(dest: &Path) -> PathBuf {
    sibling_path(dest, COMMIT_RECORD_SUFFIX).unwrap()
}

#[test]
fn same_commit_does_not_fetch() {
    let dest = fresh_dest("same_commit");
    let files = [("Bass/Bass 1.sxsnp", "<patch/>")];
    let mut fetcher = FakeFetcher::with_files("aaaaaaa", &files);

    let first = sync_factory_commit("aaaaaaa", &dest, &mut fetcher).unwrap();
    assert_eq!(
        first,
        FactorySync::Downloaded {
            commit: "aaaaaaa".into(),
            patches: 1
        }
    );
    assert_eq!(read(&dest.join("Bass").join("Bass 1.sxsnp")), "<patch/>");
    assert_eq!(read(&record_of(&dest)), "aaaaaaa");
    let calls_after_first = fetcher.calls;

    let second = sync_factory_commit("aaaaaaa", &dest, &mut fetcher).unwrap();
    assert_eq!(
        second,
        FactorySync::Unchanged {
            commit: "aaaaaaa".into()
        }
    );
    assert_eq!(fetcher.calls, calls_after_first);
}

#[test]
fn different_commit_replaces_dest() {
    let dest = fresh_dest("replace");
    let mut old = FakeFetcher::with_files("aaaaaaa", &[("Pads/Old.sxsnp", "old")]);
    sync_factory_commit("aaaaaaa", &dest, &mut old).unwrap();

    let mut new = FakeFetcher::with_files("bbbbbbb", &[("Keys/New.sxsnp", "new!")]);
    let result = sync_factory_commit("bbbbbbb", &dest, &mut new).unwrap();

    assert_eq!(
        result,
        FactorySync::Downloaded {
            commit: "bbbbbbb".into(),
            patches: 1
        }
    );
    assert!(!dest.join("Pads").exists(), "旧版のファイルが残っている");
    assert_eq!(read(&dest.join("Keys").join("New.sxsnp")), "new!");
    assert_eq!(read(&record_of(&dest)), "bbbbbbb");
    assert!(!sibling_path(&dest, TMP_SUFFIX).unwrap().exists());
}

#[test]
fn failure_midway_keeps_old_dest_and_record() {
    let dest = fresh_dest("midway");
    let mut old = FakeFetcher::with_files("aaaaaaa", &[("Pads/Old.sxsnp", "old")]);
    sync_factory_commit("aaaaaaa", &dest, &mut old).unwrap();

    let mut new = FakeFetcher::with_files(
        "bbbbbbb",
        &[("Bass/A.sxsnp", "a"), ("Bass/B%20x.sxsnp", "b")],
    );
    new.fail_on = Some("B%2520x".into());
    let error = sync_factory_commit("bbbbbbb", &dest, &mut new).unwrap_err();

    assert!(format!("{error:#}").contains("偽の通信失敗"), "{error:#}");
    assert_eq!(read(&dest.join("Pads").join("Old.sxsnp")), "old");
    assert_eq!(read(&record_of(&dest)), "aaaaaaa");
    assert!(!sibling_path(&dest, TMP_SUFFIX).unwrap().exists());
}

#[test]
fn truncated_tree_is_rejected() {
    let dest = fresh_dest("truncated");
    let mut fetcher = FakeFetcher::with_files("aaaaaaa", &[("Bass/A.sxsnp", "a")]);
    fetcher.tree = fetcher
        .tree
        .replace(r#""truncated":false"#, r#""truncated":true"#);

    let error = sync_factory_commit("aaaaaaa", &dest, &mut fetcher).unwrap_err();

    assert!(error.to_string().contains("truncated"), "{error:#}");
    assert!(!dest.exists());
    assert!(!record_of(&dest).exists());
}

#[test]
fn version_without_hash_is_error() {
    assert_eq!(commit_from_version("1.2.0.18ecb36").unwrap(), "18ecb36");
    for version in [
        "1.2.0",
        "1.2.0.18ECB36",
        "1.2.0.18ecb3",
        "",
        "1.2.0.zzzzzzz",
    ] {
        assert!(commit_from_version(version).is_err(), "{version:?}");
    }
}

#[test]
fn size_mismatch_is_error() {
    let dest = fresh_dest("size");
    let mut fetcher = FakeFetcher::with_files("aaaaaaa", &[("Bass/A.sxsnp", "abc")]);
    fetcher.tree = fetcher.tree.replace(r#""size":3"#, r#""size":4"#);

    let error = sync_factory_commit("aaaaaaa", &dest, &mut fetcher).unwrap_err();

    assert!(error.to_string().contains("大きさ"), "{error:#}");
    assert!(!dest.exists());
}

#[test]
fn tree_path_escaping_dest_is_rejected() {
    let json = r#"{"truncated":false,"tree":[{"path":"../evil.sxsnp","type":"blob","size":1}]}"#;
    assert!(parse_tree(json).is_err());
}

#[test]
fn path_is_percent_encoded_per_segment() {
    assert_eq!(encode_path("Bass/Bass 1.sxsnp"), "Bass/Bass%201.sxsnp");
    assert_eq!(
        encode_path("Keys/E&P (x).sxsnp"),
        "Keys/E%26P%20%28x%29.sxsnp"
    );
}

/// 取得回数を数えながら実際に通信する取得口。
struct CountingHttp {
    inner: HttpFetcher,
    calls: usize,
}

impl FactoryFetcher for CountingHttp {
    fn get_text(&mut self, url: &str) -> Result<String> {
        self.calls += 1;
        self.inner.get_text(url)
    }

    fn get_bytes(&mut self, url: &str) -> Result<Vec<u8>> {
        self.calls += 1;
        self.inner.get_bytes(url)
    }
}

fn count_patches(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| {
            if path.is_dir() {
                count_patches(&path)
            } else {
                usize::from(path.to_string_lossy().ends_with(PATCH_EXTENSION))
            }
        })
        .sum()
}

/// 実 plugin と GitHub を使う。既定の置き場へ取得し、2 回目は通信しないことを見る。
///
/// `CMRT_SIX_SINES_PLUGIN` で plugin のパスを差し替えられる（既定は Windows の標準インストール先）。
#[test]
#[ignore = "実 plugin と GitHub への通信が要る"]
fn real_factory_is_downloaded_once() {
    let plugin = std::env::var("CMRT_SIX_SINES_PLUGIN").unwrap_or_else(|_| {
        r"C:\Program Files\Common Files\CLAP\BaconPaul\Six Sines.clap".to_string()
    });
    let dest = cmrt_server_config::six_sines_factory_dir().unwrap();
    let mut fetcher = CountingHttp {
        inner: HttpFetcher::default(),
        calls: 0,
    };

    let first = sync_six_sines_factory_with(&plugin, &dest, &mut fetcher).unwrap();
    println!("1 回目: {first} / 取得 {} 回", fetcher.calls);
    let commit = read(&record_of(&dest));
    let patches = count_patches(&dest);
    println!("置き場 {} に {patches} 件、commit {commit}", dest.display());

    fetcher.calls = 0;
    let second = sync_six_sines_factory_with(&plugin, &dest, &mut fetcher).unwrap();
    println!("2 回目: {second} / 取得 {} 回", fetcher.calls);

    assert_eq!(patches, 216);
    assert_eq!(commit, "18ecb36");
    assert!(matches!(second, FactorySync::Unchanged { .. }));
    assert_eq!(fetcher.calls, 0);
}
