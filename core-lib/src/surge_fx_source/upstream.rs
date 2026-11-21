use crate::six_sines_factory::FactoryFetcher;
use anyhow::{bail, ensure, Context, Result};
use plugin_presets::surge_fx::{
    source_layout::{effect_classes, extract_layout, LayoutSources},
    SurgeFxParamLayout,
};
use serde::Deserialize;
use std::path::{Component, Path};

const API: &str = "https://api.github.com/repos/surge-synthesizer/surge";

#[derive(Deserialize)]
struct GitTree {
    tree: Vec<GitEntry>,
    truncated: bool,
}
#[derive(Deserialize)]
struct GitEntry {
    path: String,
    sha: String,
}

pub(super) struct Upstream<'a, F> {
    pub revision: String,
    pub sst_revision: String,
    paths: Vec<String>,
    root: &'a Path,
    fetcher: &'a mut F,
}

impl<'a, F: FactoryFetcher> Upstream<'a, F> {
    pub fn resolve(tag: &str, root: &'a Path, fetcher: &'a mut F) -> Result<Self> {
        let commit: serde_json::Value =
            serde_json::from_str(&fetcher.get_text(&format!("{API}/commits/{tag}"))?)?;
        let revision = commit["sha"]
            .as_str()
            .context("official release has no commit SHA")?
            .to_owned();
        check_sha(&revision)?;
        let tree: GitTree = serde_json::from_str(
            &fetcher.get_text(&format!("{API}/git/trees/{revision}?recursive=1"))?,
        )?;
        ensure!(!tree.truncated, "official source tree is truncated");
        let sst_revision = tree
            .tree
            .iter()
            .find(|e| e.path == "libs/sst/sst-effects")
            .context("release has no sst-effects submodule")?
            .sha
            .clone();
        check_sha(&sst_revision)?;
        Ok(Self {
            revision,
            sst_revision,
            paths: tree.tree.into_iter().map(|e| e.path).collect(),
            root,
            fetcher,
        })
    }

    fn source(&mut self, path: &str, sst: bool) -> Result<String> {
        ensure!(
            !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "unsafe upstream source path {path}"
        );
        let (repo, revision) = if sst {
            ("sst-effects", &self.sst_revision)
        } else {
            ("surge", &self.revision)
        };
        let cache = self.root.join(repo).join(revision).join(path);
        match std::fs::read_to_string(&cache) {
            Ok(source) => return Ok(source),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| cache.display().to_string()),
        }
        let url =
            format!("https://raw.githubusercontent.com/surge-synthesizer/{repo}/{revision}/{path}");
        let source = self
            .fetcher
            .get_text(&url)
            .with_context(|| format!("official source {path}"))?;
        ensure!(source.len() < 4 * 1024 * 1024, "upstream source too large");
        std::fs::create_dir_all(cache.parent().context("source cache has no parent")?)?;
        // Content-addressed files are published by rename; interrupted writes
        // never become a source file on a subsequent attempt.
        let temporary = cache.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, &source)?;
        let result = std::fs::rename(&temporary, &cache);
        let _ = std::fs::remove_file(&temporary);
        result?;
        Ok(source)
    }

    fn find_file(&self, filename: &str) -> Result<String> {
        let suffix = format!("/{filename}");
        let matches: Vec<_> = self
            .paths
            .iter()
            .filter(|p| p.starts_with("src/common/dsp/effects/") && p.ends_with(&suffix))
            .collect();
        ensure!(
            matches.len() == 1,
            "expected one official {filename}, found {}",
            matches.len()
        );
        Ok(matches[0].clone())
    }

    pub fn generate(&mut self) -> Result<Vec<SurgeFxParamLayout>> {
        let storage = self.source("src/common/SurgeStorage.h", false)?;
        let effect_base = self.source("src/common/dsp/Effect.cpp", false)?;
        let parameter = self.source("src/common/Parameter.cpp", false)?;
        let mut layouts = Vec::new();
        for effect in effect_classes(&storage, &effect_base)? {
            let class = effect
                .class_name
                .rsplit("::")
                .next()
                .context("empty class name")?;
            // Preserve the existing supported set. Audio Input depends on a
            // synth FX slot and was never supported by the preset converter.
            // This is a capability exclusion, not a copied parameter table.
            if class == "AudioInputEffect" {
                continue;
            }
            let header_path = self.find_file(&format!("{class}.h"))?;
            let header = self.source(&header_path, false)?;
            let cpp_path = self
                .find_file(&format!("{class}.cpp"))
                .or_else(|_| self.find_file(&format!("{class}Impl.h")))?;
            let implementation = self.source(&cpp_path, false)?;
            let includes = quoted_includes(&header);
            let sst_paths: Vec<_> = includes
                .iter()
                .filter_map(|i| i.strip_prefix("sst/effects/"))
                .collect();
            ensure!(sst_paths.len() <= 1, "multiple sst effect metadata headers");
            let sst = sst_paths
                .first()
                .map(|p| self.source(&format!("include/sst/effects/{p}"), true))
                .transpose()?;
            let layout = extract_layout(
                effect.fx_type,
                effect.display_name,
                LayoutSources {
                    implementation: &implementation,
                    header: &header,
                    sst_header: sst.as_deref(),
                    storage: &storage,
                    effect_base: &effect_base,
                    parameter: &parameter,
                },
            )
            .with_context(|| format!("cannot extract {} ({cpp_path})", effect.class_name))?;
            layouts.push(layout);
        }
        ensure!(!layouts.is_empty(), "no layouts extracted");
        Ok(layouts)
    }
}

fn quoted_includes(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix("#include")?.trim();
            Some(line.strip_prefix('"')?.split('"').next()?.to_owned())
        })
        .collect()
}

fn check_sha(sha: &str) -> Result<()> {
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid official git revision")
    }
    Ok(())
}
