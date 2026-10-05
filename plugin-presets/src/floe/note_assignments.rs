//! preset の参照 instrument と評価済み library の note-on 鍵域を合成する。

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::{
    floe_library_dirs,
    library_regions::Library,
    lua_regions,
    preset_layers::{self, Instrument},
};

/// Floe の library 置き場から正確な割当を取得する。未解決の参照や評価失敗は部分集合を返さない。
pub fn floe_note_assignments(path: &Path) -> Result<Vec<u8>> {
    notes_from_dirs(path, &floe_library_dirs())
}

fn notes_from_dirs(path: &Path, dirs: &[PathBuf]) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Floe preset を読めない: {}", path.display()))?;
    let layers = preset_layers::read(&bytes)?;
    let needed: BTreeSet<_> = layers
        .iter()
        .filter_map(|layer| match &layer.instrument {
            Instrument::Sampler { library, .. } => Some(*library),
            _ => None,
        })
        .collect();
    let mut libraries = Vec::new();
    let mut failures = Vec::new();
    if !needed.is_empty() {
        let mut files = Vec::new();
        for dir in dirs {
            collect_library_files(dir, &mut files, &mut failures);
        }
        files.sort();
        files.dedup();
        for file in files {
            match lua_regions::evaluate(&file) {
                Ok(library) if needed.contains(&library.id) => {
                    if libraries
                        .iter()
                        .any(|other: &Library| other.id == library.id)
                    {
                        anyhow::bail!("Floe library ID が複数の file にある: {}", file.display());
                    }
                    libraries.push(library);
                }
                Ok(_) => {}
                Err(error) => failures.push(format!("{error:#}")),
            }
        }
    }
    let mut notes = BTreeSet::new();
    for layer in layers {
        let region_notes = match layer.instrument {
            Instrument::None => continue,
            Instrument::Waveform => (0..=127).collect::<BTreeSet<u8>>(),
            Instrument::Sampler { library, id } => {
                let library = libraries
                    .iter()
                    .find(|lib| lib.id == library)
                    .with_context(|| {
                        format!(
                            "Floe library を解決できない: {library:016x}; {}",
                            failures.join("; ")
                        )
                    })?;
                let regions = library
                    .instruments
                    .get(&id)
                    .with_context(|| format!("Floe instrument を解決できない: {id}"))?;
                regions
                    .iter()
                    .filter(|region| region.note_on)
                    .flat_map(|region| region.low..region.end)
                    .collect()
            }
        };
        for note in layer.low..=layer.high {
            let mapped = i16::from(note) + layer.transpose;
            if let Ok(mapped) = u8::try_from(mapped) {
                if region_notes.contains(&mapped) {
                    notes.insert(note);
                }
            }
        }
    }
    Ok(notes.into_iter().collect())
}

fn collect_library_files(dir: &Path, files: &mut Vec<PathBuf>, failures: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!("{}: {error}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            failures.push(format!("file type を読めない: {}", path.display()));
            continue;
        };
        if kind.is_dir() {
            collect_library_files(&path, files, failures);
        } else if kind.is_file()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| {
                    let name = name.to_ascii_lowercase();
                    name == "floe.lua" || name.ends_with(".floe.lua")
                })
        {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests;
