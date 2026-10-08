use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn fixture() -> Vec<u8> {
    let xml = b"<patch/>";
    let mut chunk = b"sub3".to_vec();
    chunk.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    for length in [1u32, 2, 3, 4, 5, 6] {
        chunk.extend_from_slice(&length.to_le_bytes());
    }
    chunk.extend_from_slice(xml);
    chunk.extend(0..21);
    let mut raw = vec![0; 60];
    raw[..4].copy_from_slice(b"CcnK");
    raw[8..12].copy_from_slice(b"FPCh");
    raw[16..20].copy_from_slice(b"cjs3");
    raw[56..60].copy_from_slice(&(chunk.len() as u32).to_be_bytes());
    raw.extend(chunk);
    raw
}

struct PatchFile(PathBuf);

impl PatchFile {
    fn new(extension: &str, bytes: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cmrt-patch-prepare-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for PatchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn prepared_state_is_the_exact_surge_chunk() {
    let raw = fixture();
    for extension in ["fxp", "FXP", "FxP"] {
        let file = PatchFile::new(extension, &raw);
        assert_eq!(
            prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &file.0).unwrap(),
            raw[60..]
        );
    }
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PatchStateError>();
}

#[test]
fn input_contract_is_checked_before_disk_read() {
    let path = if cfg!(windows) {
        Path::new("X:/missing/patch.fxp")
    } else {
        Path::new("/missing/patch.fxp")
    };
    match prepare_clap_patch_state("other", path).unwrap_err() {
        PatchStateError::UnsupportedPlugin { plugin_id } => assert_eq!(plugin_id, "other"),
        error => panic!("{error:?}"),
    }
    for path in ["relative.fxp", "patches/test.FXP"] {
        assert!(matches!(
            prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, Path::new(path)),
            Err(PatchStateError::InvalidPath { .. })
        ));
    }
    let file = PatchFile::new("xml", b"<patch/>");
    assert!(matches!(
        prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &file.0),
        Err(PatchStateError::UnsupportedFormat { .. })
    ));
    assert!(matches!(
        prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &file.0.with_extension("fxp")),
        Err(PatchStateError::Io { .. })
    ));
}

#[test]
fn disappeared_file_retains_io_source_and_path() {
    let file = PatchFile::new("fxp", &fixture());
    std::fs::remove_file(&file.0).unwrap();
    let error = prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &file.0).unwrap_err();
    assert!(error.source().is_some());
    match error {
        PatchStateError::Io { path, source } => {
            assert_eq!(path, file.0);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        error => panic!("{error:?}"),
    }
}

#[test]
fn truncated_headers_and_chunks_return_errors_without_panicking() {
    let raw = fixture();
    let path = Path::new("fixture.fxp");
    for length in 0..raw.len() {
        assert!(
            matches!(
                decode_surge_fxp(&raw[..length], path),
                Err(PatchStateError::InvalidData { .. })
            ),
            "length {length}"
        );
    }
    for length in 60..92 {
        let mut truncated = raw[..length].to_vec();
        truncated[56..60].copy_from_slice(&((length - 60) as u32).to_be_bytes());
        assert!(
            decode_surge_fxp(&truncated, path).is_err(),
            "length {length}"
        );
    }
}

#[test]
fn invalid_magic_ids_and_declared_lengths_are_rejected() {
    let path = Path::new("fixture.fxp");
    for offset in [0, 8, 16, 60] {
        let mut raw = fixture();
        raw[offset] ^= 1;
        assert!(decode_surge_fxp(&raw, path).is_err(), "offset {offset}");
    }
    for length in [0, 31, u32::MAX] {
        let mut raw = fixture();
        raw[56..60].copy_from_slice(&length.to_be_bytes());
        assert!(decode_surge_fxp(&raw, path).is_err());
    }
    let mut raw = fixture();
    raw.push(0);
    assert!(decode_surge_fxp(&raw, path).is_err());
    // Each sub3 length is checked, including XML and all six wavetables.
    for offset in (64..92).step_by(4) {
        let mut raw = fixture();
        raw[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_surge_fxp(&raw, path).is_err(), "offset {offset}");
    }
    let file = PatchFile::new("fxp", b"arbitrary raw state");
    assert!(matches!(
        prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &file.0),
        Err(PatchStateError::InvalidData { .. })
    ));
}

/// Opt-in read-only comparison against the user's existing catalog, never a fixture copy.
#[test]
#[ignore = "requires CMRT_PATCH_CATALOG pointing to an existing catalog"]
fn real_catalog_surge_bytes_match_the_previous_normal_decoder() {
    let catalog_path = std::env::var_os("CMRT_PATCH_CATALOG").expect("set CMRT_PATCH_CATALOG");
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(catalog_path).unwrap()).unwrap();
    let plugin = catalog["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["plugin_id"] == SURGE_XT_PLUGIN_ID)
        .unwrap();
    let base = Path::new(plugin["base"]["Shared"].as_str().unwrap());
    let key = format!("id:{SURGE_XT_PLUGIN_ID}");
    let mut count = 0;
    let mut rejected = 0;
    for patch in catalog["patches"].as_array().unwrap() {
        let reference = &patch["audio"]["reference"];
        if reference["plugin"].as_str() != Some(&key) {
            continue;
        }
        let path = base.join(reference["display"].as_str().unwrap());
        let prepared = match crate::prepare_clap_patch_state(SURGE_XT_PLUGIN_ID, &path) {
            Ok(prepared) => prepared,
            Err(error @ PatchStateError::InvalidData { .. }) => {
                // Catalog membership does not guarantee the approved FXP input contract.
                // Report rejected data explicitly and never send it to the old decoder.
                eprintln!("Rejected catalog input outside the supported contract: {error}");
                rejected += 1;
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        // New validation runs first so malformed data never reaches legacy slicing.
        let raw = std::fs::read(&path).unwrap();
        let chunk_size = u32::from_be_bytes(raw[56..60].try_into().unwrap()) as usize;
        let legacy_end = (60 + chunk_size).min(raw.len());
        assert_eq!(prepared, raw[60..legacy_end], "{}", path.display());
        count += 1;
    }
    assert!(count > 0);
    eprintln!("Compared {count} valid catalog Surge FXP files; rejected {rejected} outside the input contract; no plugin/device access");
}
