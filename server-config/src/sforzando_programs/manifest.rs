use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use xmltree::{Element, XMLNode};

use super::{canonical_key, is_sfz, path_is_within, SforzandoProgramRef};

pub(super) struct ParsedManifest {
    pub(super) programs: Vec<SforzandoProgramRef>,
    pub(super) diagnostics: Vec<String>,
}

pub(super) fn manifests_near(path: &Path) -> Vec<PathBuf> {
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let mut manifests = Vec::new();
    let mut seen = HashSet::new();
    for directory in start.ancestors() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            let Some(name) = candidate.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !candidate.is_file() || !name.to_ascii_lowercase().ends_with(".bank.xml") {
                continue;
            }
            let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
            if seen.insert(canonical_key(&canonical)) {
                manifests.push(canonical);
            }
        }
    }
    manifests.sort_by_key(|path| canonical_key(path));
    manifests
}

pub(super) fn read_manifest(path: &Path) -> anyhow::Result<ParsedManifest> {
    let text = std::fs::read_to_string(path)?;
    // Plogue bank manifests contain a sibling <Key> before <AriaBank>, so they are not a
    // single-root XML document. Wrap the document content solely for parsing.
    let body = strip_xml_declaration(&text);
    let wrapped = format!("<AriaManifest>{body}</AriaManifest>");
    let wrapper = Element::parse(Cursor::new(wrapped.as_bytes()))?;
    let bank = child_elements(&wrapper)
        .find(|element| element.name == "AriaBank")
        .ok_or_else(|| anyhow::anyhow!("AriaBank element がない"))?;
    let bank_id = required_attr(bank, "id", "AriaBank")?;
    let bank_version = required_attr(bank, "version", "AriaBank")?;
    let bank_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest directory がない"))?;

    let mut programs = Vec::new();
    let mut diagnostics = Vec::new();
    for program in child_elements(bank).filter(|element| element.name == "AriaProgram") {
        let Some(program_name) = program
            .attributes
            .get("name")
            .filter(|name| !name.is_empty())
        else {
            diagnostics.push(format!(
                "ARIA bank manifest '{}' の AriaProgram に name がない",
                path.display()
            ));
            continue;
        };
        let elements = child_elements(program)
            .filter(|element| element.name == "AriaElement")
            .collect::<Vec<_>>();
        if elements.is_empty() {
            diagnostics.push(format!(
                "ARIA program '{program_name}' に AriaElement がない ({})",
                path.display()
            ));
            continue;
        }
        for element in elements {
            let Some(relative) = element
                .attributes
                .get("path")
                .filter(|value| !value.is_empty())
            else {
                diagnostics.push(format!(
                    "ARIA program '{program_name}' の AriaElement に path がない ({})",
                    path.display()
                ));
                continue;
            };
            let relative_path = Path::new(relative);
            if relative_path.is_absolute() {
                diagnostics.push(format!(
                    "ARIA program '{program_name}' の absolute path を拒否: '{relative}'"
                ));
                continue;
            }
            let joined = bank_dir.join(relative_path);
            let canonical = match std::fs::canonicalize(&joined) {
                Ok(canonical) => canonical,
                Err(error) => {
                    diagnostics.push(format!(
                        "ARIA program '{program_name}' の SFZ が実在しない '{}': {error}",
                        joined.display()
                    ));
                    continue;
                }
            };
            if !path_is_within(&canonical, bank_dir) {
                diagnostics.push(format!(
                    "ARIA program '{program_name}' の bank root 外参照を拒否: '{}'",
                    canonical.display()
                ));
                continue;
            }
            if !is_sfz(&canonical) {
                diagnostics.push(format!(
                    "ARIA program '{program_name}' の非 SFZ element を除外: '{}'",
                    canonical.display()
                ));
                continue;
            }
            programs.push(SforzandoProgramRef {
                sfz_path: canonical,
                bank_id: bank_id.to_string(),
                bank_version: bank_version.to_string(),
                program_name: program_name.to_string(),
                source: format!("bank manifest '{}'", path.display()),
            });
        }
    }
    Ok(ParsedManifest {
        programs,
        diagnostics,
    })
}

fn child_elements(element: &Element) -> impl Iterator<Item = &Element> {
    element.children.iter().filter_map(|node| match node {
        XMLNode::Element(element) => Some(element),
        _ => None,
    })
}

fn required_attr<'a>(element: &'a Element, name: &str, owner: &str) -> anyhow::Result<&'a str> {
    element
        .attributes
        .get(name)
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("{owner}/@{name} がない"))
}

fn strip_xml_declaration(text: &str) -> &str {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with("<?xml") {
        trimmed
            .find("?>")
            .map(|end| &trimmed[end + 2..])
            .unwrap_or(trimmed)
    } else {
        trimmed
    }
}
