//! `.sfz` を header の継承込みで region ごとの opcode 一覧へ展開する。
//!
//! `<control>` / `<global>` / `<master>` / `<group>` の opcode を下位の region へ引き継ぐ。
//! それ以外の header（`<curve>` `<effect>` など）の opcode は捨てる。
//! `#include` はルートの `.sfz` があるディレクトリを基準に解決し、`#define` の `$変数` は
//! 定義より後の行で展開する。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;

use super::sample_weight::{normalize, quoted, read_sfz_text, value_len};

/// `#include` をたどる深さの上限。循環した `#include` はここで打ち切る。
const MAX_INCLUDE_DEPTH: usize = 16;

/// 継承の上位から順に並べた header。
const LEVELS: [&str; 4] = ["control", "global", "master", "group"];

/// 1 region に効く opcode。header から継承したものを含み、region 自身の値が勝つ。
pub(super) type SfzRegion = BTreeMap<String, String>;

/// `path` の `.sfz` の region を出現順に返す。ルートの `.sfz` を読めないときだけ `Err` を返す。
pub(super) fn sfz_regions(path: &Path) -> anyhow::Result<Vec<SfzRegion>> {
    read_regions(path, false)
}

/// 全割当の取得には include の欠損・循環を許容しない。
pub(super) fn complete_sfz_regions(path: &Path) -> anyhow::Result<Vec<SfzRegion>> {
    read_regions(path, true)
}

fn read_regions(path: &Path, strict: bool) -> anyhow::Result<Vec<SfzRegion>> {
    let text =
        read_sfz_text(path).with_context(|| format!("sfz を読めない: {}", path.display()))?;
    let mut walker = Walker {
        root_dir: path.parent().unwrap_or_else(|| Path::new("")),
        defines: Vec::new(),
        levels: Default::default(),
        current: Current::Ignored,
        region: None,
        regions: Vec::new(),
        strict,
    };
    walker.walk_text(&text, 0)?;
    walker.flush_region();
    Ok(walker.regions)
}

enum Current {
    Level(usize),
    Region,
    Ignored,
}

struct Walker<'a> {
    root_dir: &'a Path,
    /// `#define` の名前（`$` 込み）と値。長い名前から置換するため展開時に並べ替える。
    defines: Vec<(String, String)>,
    levels: [SfzRegion; LEVELS.len()],
    current: Current,
    region: Option<SfzRegion>,
    regions: Vec<SfzRegion>,
    strict: bool,
}

impl Walker<'_> {
    fn walk_text(&mut self, text: &str, depth: usize) -> anyhow::Result<()> {
        for line in text.lines() {
            let line = line.find("//").map_or(line, |end| &line[..end]);
            if let Some(define) = line.trim_start().strip_prefix("#define") {
                self.define(define);
                continue;
            }
            let line = self.expand_defines(line);
            self.walk_line(&line, depth)?;
        }
        Ok(())
    }

    fn define(&mut self, rest: &str) {
        let mut parts = rest.split_whitespace();
        if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
            self.defines.retain(|(defined, _)| defined != name);
            self.defines.push((name.to_string(), value.to_string()));
        }
    }

    fn expand_defines(&self, line: &str) -> String {
        if self.defines.is_empty() || !line.contains('$') {
            return line.to_string();
        }
        let mut defines = self.defines.iter().collect::<Vec<_>>();
        defines.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        defines
            .into_iter()
            .fold(line.to_string(), |line, (name, value)| {
                line.replace(name.as_str(), value)
            })
    }

    fn walk_line(&mut self, line: &str, depth: usize) -> anyhow::Result<()> {
        let mut pos = 0;
        while pos < line.len() {
            let rest = &line[pos..];
            if let Some(after) = rest.strip_prefix("#include") {
                pos += "#include".len();
                if let Some((include, consumed)) = quoted(after) {
                    pos += consumed;
                    self.walk_include(include, depth)?;
                } else if self.strict {
                    anyhow::bail!("sfz の include 指定が不正: {line}");
                }
            } else if let Some(end) = rest.strip_prefix('<').and_then(|body| body.find('>')) {
                self.header(&rest[1..=end]);
                pos += end + 2;
            } else if let Some(name_len) = opcode_name_len(line, pos) {
                let start = pos + name_len + 1;
                let end = start + value_len(&line[start..]);
                self.opcode(&line[pos..pos + name_len], line[start..end].trim());
                pos = end;
            } else {
                pos += rest.chars().next().map_or(1, char::len_utf8);
            }
        }
        Ok(())
    }

    fn walk_include(&mut self, include: &str, depth: usize) -> anyhow::Result<()> {
        if depth >= MAX_INCLUDE_DEPTH {
            if self.strict {
                anyhow::bail!("sfz の include 深さ超過または循環: {include}");
            }
            return Ok(());
        }
        let path = normalize(self.root_dir, &[include]);
        match read_sfz_text(&path) {
            Ok(text) => self.walk_text(&text, depth + 1)?,
            Err(error) if self.strict => {
                return Err(error)
                    .with_context(|| format!("sfz include を読めない: {}", path.display()));
            }
            Err(_) => {}
        }
        Ok(())
    }

    fn header(&mut self, name: &str) {
        self.flush_region();
        let name = name.trim().to_ascii_lowercase();
        if name == "region" {
            self.region = Some(SfzRegion::new());
            self.current = Current::Region;
        } else if let Some(level) = LEVELS.iter().position(|level| *level == name) {
            // `<control>` は継承の階層の外にあり、下位の header を消さない。Plogue の CR-909 は
            // `<global>` の後で `<control>` から始まる file を include し、`<global>` が効き続ける前提。
            let cleared = if name == "control" {
                0..1
            } else {
                level..LEVELS.len()
            };
            for inherited in &mut self.levels[cleared] {
                inherited.clear();
            }
            self.current = Current::Level(level);
        } else {
            self.current = Current::Ignored;
        }
    }

    fn opcode(&mut self, name: &str, value: &str) {
        let target = match self.current {
            Current::Level(level) => &mut self.levels[level],
            Current::Region => match &mut self.region {
                Some(region) => region,
                None => return,
            },
            Current::Ignored => return,
        };
        target.insert(name.to_ascii_lowercase(), value.to_string());
    }

    fn flush_region(&mut self) {
        let Some(own) = self.region.take() else {
            return;
        };
        let mut merged = SfzRegion::new();
        for inherited in &self.levels {
            merged.extend(inherited.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        merged.extend(own);
        self.regions.push(merged);
    }
}

/// `pos` から `英数字_` が 1 文字以上並び `=` が続くなら、その名前の byte 数。
///
/// opcode は行頭・空白・`>` の直後でだけ認める。
fn opcode_name_len(line: &str, pos: usize) -> Option<usize> {
    let at_boundary = line[..pos]
        .chars()
        .next_back()
        .is_none_or(|previous| previous.is_whitespace() || previous == '>');
    if !at_boundary {
        return None;
    }
    let rest = &line[pos..];
    let name_len = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(rest.len());
    (name_len > 0 && rest[name_len..].starts_with('=')).then_some(name_len)
}

#[cfg(test)]
mod tests;
