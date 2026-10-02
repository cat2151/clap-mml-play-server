//! `.ariax`（ARIA の preset。平文の AriaSave XML）を、近傍の bank manifest に登録された
//! program として解決する。
//!
//! `.ariax` の `Slot@name/@bankId/@version` がそのまま Sforzando へ渡る program 座標になる。
//! 近傍の `*.bank.xml` の `AriaProgram` と一致したものだけを採用し、一致しなければエラーにする。

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use xmltree::{Element, XMLNode};

use super::{canonical_key, manifest, SforzandoProgramRef};

/// 近傍の bank manifest に登録された program を指す `.ariax`。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SforzandoPresetRef {
    pub ariax_path: PathBuf,
    /// `.ariax` の中身（AriaSave XML）。
    pub xml: String,
    /// `Slot` の座標と一致した manifest の program。
    pub program: SforzandoProgramRef,
}

pub(super) fn is_ariax(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ariax"))
}

/// `.ariax` を読み、`Slot` の program 座標を近傍の bank manifest と照合する。
pub fn resolve_sforzando_preset(path: &Path) -> anyhow::Result<SforzandoPresetRef> {
    let canonical = crate::lexical_absolute(path).map_err(|error| {
        anyhow::anyhow!(
            ".ariax path を絶対パスにできない '{}': {error}",
            path.display()
        )
    })?;
    if !is_ariax(&canonical) {
        anyhow::bail!(
            "ARIA preset は .ariax file でなければならない: '{}'",
            canonical.display()
        );
    }
    let xml = std::fs::read_to_string(&canonical)
        .map_err(|error| anyhow::anyhow!(".ariax を読めない '{}': {error}", canonical.display()))?;
    let slot = slot_coordinates(&xml)
        .map_err(|error| anyhow::anyhow!("{error:#} '{}'", canonical.display()))?;

    let mut inspected = Vec::new();
    let mut manifest_errors = Vec::new();
    for manifest_path in manifest::manifests_near(&canonical) {
        inspected.push(manifest_path.display().to_string());
        match manifest::read_manifest(&manifest_path) {
            Ok(parsed) => {
                if let Some(program) = find_program(&slot, &parsed.programs) {
                    return Ok(SforzandoPresetRef {
                        ariax_path: canonical,
                        xml,
                        program: program.clone(),
                    });
                }
            }
            Err(error) => manifest_errors.push(format!("{}: {error:#}", manifest_path.display())),
        }
    }

    let mut manifests = if inspected.is_empty() {
        "近傍に *.bank.xml がない".to_string()
    } else {
        format!("一致する AriaProgram がない: {}", inspected.join(" / "))
    };
    if !manifest_errors.is_empty() {
        manifests.push_str(&format!("; parse error: {}", manifest_errors.join(" / ")));
    }
    anyhow::bail!(
        ".ariax の Slot 座標 (name='{}' bankId='{}' version='{}') を ARIA bank manifest で解決できない '{}': {manifests}",
        slot.name,
        slot.bank_id,
        slot.version,
        canonical.display()
    )
}

/// catalog に載せる `.ariax` を選ぶ。[`resolve_sforzando_preset`] と同じく、`Slot` の座標が
/// 近傍の bank manifest の program と一致するものだけを残す。
///
/// 一致しない・読めない `.ariax`（別の環境の program を指す preset 等）は、user の側で
/// 直せないので notice を出さずに除外する。
pub(super) fn listable_presets(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut programs_by_manifest = HashMap::<String, Vec<SforzandoProgramRef>>::new();
    files
        .into_iter()
        .filter(|path| {
            let Some(slot) = std::fs::read_to_string(path)
                .ok()
                .and_then(|xml| slot_coordinates(&xml).ok())
            else {
                return false;
            };
            manifest::manifests_near(path).iter().any(|manifest_path| {
                let programs = programs_by_manifest
                    .entry(canonical_key(manifest_path))
                    .or_insert_with(|| {
                        manifest::read_manifest(manifest_path)
                            .map(|parsed| parsed.programs)
                            .unwrap_or_default()
                    });
                find_program(&slot, programs).is_some()
            })
        })
        .collect()
}

fn find_program<'a>(
    slot: &SlotCoordinates,
    programs: &'a [SforzandoProgramRef],
) -> Option<&'a SforzandoProgramRef> {
    programs.iter().find(|program| {
        program.bank_id == slot.bank_id
            && program.bank_version == slot.version
            && program.program_name == slot.name
    })
}

struct SlotCoordinates {
    name: String,
    bank_id: String,
    version: String,
}

/// `AriaSave` の program slot（`Slot` で `id` が無いか `0`）の座標。
fn slot_coordinates(xml: &str) -> anyhow::Result<SlotCoordinates> {
    let root = Element::parse(Cursor::new(xml.as_bytes()))
        .map_err(|error| anyhow::anyhow!(".ariax の AriaSave XML が不正: {error}"))?;
    if root.name != "AriaSave" {
        anyhow::bail!(
            ".ariax の root element が AriaSave ではない: '{}'",
            root.name
        );
    }
    let slot = find_program_slot(&root)
        .ok_or_else(|| anyhow::anyhow!(".ariax に Slot id=\"0\" が無い"))?;
    let attr = |name: &str| {
        slot.attributes
            .get(name)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!(".ariax の Slot/@{name} がない"))
    };
    Ok(SlotCoordinates {
        name: attr("name")?,
        bank_id: attr("bankId")?,
        version: attr("version")?,
    })
}

fn find_program_slot(element: &Element) -> Option<&Element> {
    if element.name == "Slot" && element.attributes.get("id").is_none_or(|id| id == "0") {
        return Some(element);
    }
    element.children.iter().find_map(|node| match node {
        XMLNode::Element(child) => find_program_slot(child),
        _ => None,
    })
}
