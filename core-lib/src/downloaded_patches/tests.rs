use super::*;

fn profile(plugin_path: &str, plugin_id: Option<&str>) -> PluginProfile {
    PluginProfile {
        plugin_path: plugin_path.to_string(),
        plugin_id: plugin_id.map(str::to_string),
        patches_dirs: None,
    }
}

fn installed() -> BTreeMap<String, PluginProfile> {
    BTreeMap::from([
        (
            "Six Sines".to_string(),
            profile(r"D:\clap\Six Sines.clap", Some(crate::SIX_SINES_PLUGIN_ID)),
        ),
        (
            "Surge XT".to_string(),
            profile(r"D:\clap\Surge XT.clap", Some(crate::SURGE_XT_PLUGIN_ID)),
        ),
    ])
}

#[test]
fn only_plugins_that_need_a_download_are_synced() {
    let mut called = Vec::new();
    let downloads = prepare_downloaded_patches_with(installed(), |plugin_path| {
        called.push(plugin_path.to_string());
        Ok(FactorySync::Unchanged {
            commit: "18ecb36".to_string(),
        })
    });
    assert_eq!(called, vec![r"D:\clap\Six Sines.clap".to_string()]);
    assert_eq!(downloads.len(), 1);
    assert_eq!(
        downloads[0].to_string(),
        "Six Sines: 音色の取得: 取得済み (commit 18ecb36)"
    );
}

#[test]
fn a_failed_download_is_reported_as_a_line_instead_of_an_error() {
    let downloads =
        prepare_downloaded_patches_with(installed(), |_| Err(anyhow::anyhow!("offline")));
    assert_eq!(downloads.len(), 1);
    assert_eq!(
        downloads[0].to_string(),
        "Six Sines: 音色の取得に失敗: offline"
    );
}

#[test]
fn nothing_is_synced_without_six_sines() {
    let mut profiles = installed();
    profiles.remove("Six Sines");
    let downloads = prepare_downloaded_patches_with(profiles, |_| panic!("取得してはいけない"));
    assert!(downloads.is_empty());
}
