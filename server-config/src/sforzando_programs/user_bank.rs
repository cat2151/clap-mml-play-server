use std::path::{Path, PathBuf};

use super::{is_sfz, strip_prefix_portable, SforzandoProgramRef};

// These coordinates are an ARIA Windows user-bank convention observed together with the
// registry-backed root. They are never used when that registry value is absent, and are not
// exposed as cross-platform defaults.
#[cfg(windows)]
const WINDOWS_USER_BANK_ID: &str = "5000";
#[cfg(windows)]
const WINDOWS_USER_BANK_VERSION: &str = "1000";

pub(super) struct UserBankLookup {
    pub(super) source: Option<UserBankSource>,
    pub(super) error: Option<String>,
}

pub(super) struct UserBankSource {
    pub(super) root: PathBuf,
    bank_id: String,
    bank_version: String,
}

impl UserBankSource {
    #[cfg(test)]
    pub(super) fn fixture(root: PathBuf, bank_id: &str, bank_version: &str) -> Self {
        Self {
            root,
            bank_id: bank_id.to_string(),
            bank_version: bank_version.to_string(),
        }
    }

    pub(super) fn program_for(&self, canonical_path: &Path) -> Option<SforzandoProgramRef> {
        if !is_sfz(canonical_path) {
            return None;
        }
        let relative = strip_prefix_portable(canonical_path, &self.root)?;
        if relative.as_os_str().is_empty() {
            return None;
        }
        let mut name = relative;
        name.set_extension("");
        let program_name = name.to_string_lossy().replace('\\', "/");
        Some(SforzandoProgramRef {
            sfz_path: canonical_path.to_path_buf(),
            bank_id: self.bank_id.clone(),
            bank_version: self.bank_version.clone(),
            program_name,
            source: format!("Windows registry user_files_dir ({})", self.root.display()),
        })
    }
}

#[cfg(windows)]
pub(super) fn read_user_bank_source() -> UserBankLookup {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const KEY: &str = r"Software\Plogue Art et Technologie, Inc\Aria";
    const VALUE: &str = "user_files_dir";

    let result = (|| -> anyhow::Result<UserBankSource> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(KEY)
            .map_err(|error| anyhow::anyhow!("HKCU\\{KEY} を読めない: {error}"))?;
        let value: String = key
            .get_value(VALUE)
            .map_err(|error| anyhow::anyhow!("HKCU\\{KEY}\\{VALUE} を読めない: {error}"))?;
        let root = std::fs::canonicalize(value.trim()).map_err(|error| {
            anyhow::anyhow!(
                "user_files_dir '{}' を canonicalize できない: {error}",
                value.trim()
            )
        })?;
        if !root.is_dir() {
            anyhow::bail!("user_files_dir が directory ではない: '{}'", root.display());
        }
        Ok(UserBankSource {
            root,
            bank_id: WINDOWS_USER_BANK_ID.to_string(),
            bank_version: WINDOWS_USER_BANK_VERSION.to_string(),
        })
    })();
    match result {
        Ok(source) => UserBankLookup {
            source: Some(source),
            error: None,
        },
        Err(error) => UserBankLookup {
            source: None,
            error: Some(format!("{error:#}")),
        },
    }
}

#[cfg(not(windows))]
pub(super) fn read_user_bank_source() -> UserBankLookup {
    UserBankLookup {
        source: None,
        error: Some(
            "この OS では ARIA user_files_dir の program source を自動解決できない".to_string(),
        ),
    }
}
