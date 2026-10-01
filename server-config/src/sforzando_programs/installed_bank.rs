//! ARIA installed banks registered in the Windows registry.
//!
//! Each `Aria\Products\<id>` names a vendor/product pair whose own key lists `Banks\*\bank_path`.
//! A product without `Banks` (sforzando itself) is skipped silently.

use std::path::PathBuf;

/// One ARIA product as registered, with the `bank_path` values found under its `Banks` key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AriaProduct {
    pub(super) product: String,
    pub(super) bank_paths: Vec<PathBuf>,
}

pub(super) struct InstalledBankLookup {
    pub(super) products: Vec<AriaProduct>,
    pub(super) error: Option<String>,
}

/// Bank directories (canonical) and notices for registered bank files that are gone.
pub(super) fn installed_bank_roots(products: &[AriaProduct]) -> (Vec<PathBuf>, Vec<String>) {
    let mut roots = Vec::new();
    let mut notices = Vec::new();
    for product in products {
        for bank_path in &product.bank_paths {
            let root = bank_path
                .is_file()
                .then(|| bank_path.parent())
                .flatten()
                .and_then(|dir| crate::lexical_absolute(dir).ok());
            match root {
                Some(root) => roots.push(root),
                None => notices.push(format!(
                    "ARIA installed bank '{}' の bank_path が無い '{}'; 再インストールすると一覧に出る",
                    product.product,
                    bank_path.display()
                )),
            }
        }
    }
    (roots, notices)
}

#[cfg(windows)]
pub(super) fn read_installed_banks() -> InstalledBankLookup {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    const PRODUCTS: &str = r"SOFTWARE\Plogue Art et Technologie, Inc\Aria\Products";

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let products_key = match hklm.open_subkey(PRODUCTS) {
        Ok(key) => key,
        Err(error) => {
            return InstalledBankLookup {
                products: Vec::new(),
                error: Some(format!("HKLM\\{PRODUCTS} を読めない: {error}")),
            }
        }
    };
    let products = products_key
        .enum_keys()
        .flatten()
        .filter_map(|id| {
            let entry = products_key.open_subkey(&id).ok()?;
            let vendor: String = entry.get_value("vendor").ok()?;
            let product: String = entry.get_value("product").ok()?;
            let banks = hklm
                .open_subkey(format!(r"SOFTWARE\{vendor}\{product}\Banks"))
                .ok()?;
            let bank_paths = banks
                .enum_keys()
                .flatten()
                .filter_map(|bank| {
                    let value: String =
                        banks.open_subkey(&bank).ok()?.get_value("bank_path").ok()?;
                    Some(PathBuf::from(value.trim()))
                })
                .collect();
            Some(AriaProduct {
                product,
                bank_paths,
            })
        })
        .collect();
    InstalledBankLookup {
        products,
        error: None,
    }
}

#[cfg(not(windows))]
pub(super) fn read_installed_banks() -> InstalledBankLookup {
    InstalledBankLookup {
        products: Vec::new(),
        error: Some("この OS では ARIA installed bank を自動解決できない".to_string()),
    }
}
