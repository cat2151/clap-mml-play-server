//! `.sfz` が参照する sample の数と総容量を、plugin を起動せずに数える。
//!
//! offline render は sample を全量読むので、ここで数えた総容量がロード時間の目安になる。
//! `#define` / `$変数` の展開はしない。

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;

/// `#include` をたどる深さの上限。循環した `#include` はここで打ち切る。
const MAX_INCLUDE_DEPTH: usize = 16;

/// `.sfz` が参照する sample の集計。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SfzSampleWeight {
    /// 重複を除いた sample の参照数。存在しない sample も含む。
    pub files: u32,
    /// 存在する sample の総バイト数。
    pub bytes: u64,
    /// 見つからない sample の参照数。sample か `default_path=` に `$` を含む参照は、
    /// 展開しないと判定できないので数えない（Plogue の bank は `$sample_dir` を使う）。
    pub missing: u32,
}

impl SfzSampleWeight {
    /// sample を 1 件以上参照し、そのどれも見つからない。どの MIDI を送っても鳴らない。
    pub fn all_samples_missing(&self) -> bool {
        self.files > 0 && self.missing == self.files
    }
}

/// `path` の `.sfz` が参照する sample を数える。
///
/// `#include` と `default_path=` はルートの `.sfz` があるディレクトリを基準に解決する。
/// `*` で始まる内蔵波形は数えない。ルートの `.sfz` を読めないときだけ `Err` を返し、
/// 読めない `#include` は無視する。
pub fn sfz_sample_weight(path: &Path) -> anyhow::Result<SfzSampleWeight> {
    scan_sfz(path).map(|scan| scan.weight)
}

/// [`sfz_sample_weight`] と同じ走査で集めた、sample の集計と読めた `#include` 先。
pub(super) struct SfzScan {
    pub weight: SfzSampleWeight,
    /// 字句的に畳んだだけのパス（canonicalize していない）。孫以下の `#include` も含む。
    pub includes: Vec<PathBuf>,
}

pub(super) fn scan_sfz(path: &Path) -> anyhow::Result<SfzScan> {
    let text =
        read_sfz_text(path).with_context(|| format!("sfz を読めない: {}", path.display()))?;
    let root_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut walker = Walker {
        root_dir,
        default_path: String::new(),
        seen: HashSet::new(),
        weight: SfzSampleWeight::default(),
        includes: Vec::new(),
    };
    walker.walk_text(&text, 0);
    Ok(SfzScan {
        weight: walker.weight,
        includes: walker.includes,
    })
}

fn read_sfz_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

struct Walker<'a> {
    root_dir: &'a Path,
    default_path: String,
    seen: HashSet<PathBuf>,
    weight: SfzSampleWeight,
    includes: Vec<PathBuf>,
}

impl Walker<'_> {
    fn walk_text(&mut self, text: &str, depth: usize) {
        for line in text.lines() {
            let line = line.find("//").map_or(line, |end| &line[..end]);
            self.walk_line(line, depth);
        }
    }

    fn walk_line(&mut self, line: &str, depth: usize) {
        let mut pos = 0;
        while pos < line.len() {
            let rest = &line[pos..];
            if let Some(after) = rest.strip_prefix("#include") {
                pos += "#include".len();
                if let Some((include, consumed)) = quoted(after) {
                    pos += consumed;
                    self.walk_include(include, depth);
                }
            } else if let Some(value) = opcode_value(line, pos, "sample=") {
                pos = value.end;
                self.add_sample(&line[value.start..value.end]);
            } else if let Some(value) = opcode_value(line, pos, "default_path=") {
                pos = value.end;
                self.default_path = line[value.start..value.end].trim().to_string();
            } else {
                pos += rest.chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    fn walk_include(&mut self, include: &str, depth: usize) {
        if depth >= MAX_INCLUDE_DEPTH {
            return;
        }
        let path = normalize(self.root_dir, &[include]);
        if let Ok(text) = read_sfz_text(&path) {
            self.includes.push(path);
            self.walk_text(&text, depth + 1);
        }
    }

    fn add_sample(&mut self, sample: &str) {
        let sample = sample.trim();
        if sample.is_empty() || sample.starts_with('*') {
            return;
        }
        let path = normalize(self.root_dir, &[&self.default_path, sample]);
        if !self.seen.insert(path.clone()) {
            return;
        }
        self.weight.files += 1;
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => self.weight.bytes += metadata.len(),
            _ if !sample.contains('$') && !self.default_path.contains('$') => {
                self.weight.missing += 1
            }
            _ => {}
        }
    }
}

/// `"..."` で囲まれた文字列と、閉じ引用符までに消費した byte 数。
fn quoted(text: &str) -> Option<(&str, usize)> {
    let leading = text.len() - text.trim_start().len();
    let body = text[leading..].strip_prefix('"')?;
    let end = body.find('"')?;
    Some((&body[..end], leading + 1 + end + 1))
}

/// `pos` から始まる opcode `name`（`=` を含む）の値の範囲。
///
/// opcode は行頭・空白・`>` の直後でだけ認める。値は空白を含みうるので、
/// 「次の `空白+英数字_=`」「`<`」「`#`」「行末」の最初のものまでを値とする。
fn opcode_value(line: &str, pos: usize, name: &str) -> Option<std::ops::Range<usize>> {
    if !line[pos..].starts_with(name) {
        return None;
    }
    let at_boundary = line[..pos]
        .chars()
        .next_back()
        .is_none_or(|previous| previous.is_whitespace() || previous == '>');
    if !at_boundary {
        return None;
    }
    let start = pos + name.len();
    let end = start + value_len(&line[start..]);
    Some(start..end)
}

fn value_len(value: &str) -> usize {
    for (index, ch) in value.char_indices() {
        if ch == '<' || ch == '#' {
            return index;
        }
        if ch.is_whitespace() && starts_next_opcode(&value[index..]) {
            return index;
        }
    }
    value.len()
}

/// 空白に続いて `英数字_` が 1 文字以上並び、`=` が来るか。
fn starts_next_opcode(text: &str) -> bool {
    let name = text.trim_start();
    let name_len = name
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(name.len());
    name_len > 0 && name[name_len..].starts_with('=')
}

/// `base` に `parts` を順に連結し、`\` を区切りとして扱い、`.` と `..` を字句的に畳む。
fn normalize(base: &Path, parts: &[&str]) -> PathBuf {
    let mut path = base.to_path_buf();
    for part in parts {
        for piece in part.split(['/', '\\']) {
            match piece {
                "" | "." => {}
                ".." => {
                    if !matches!(
                        path.components().next_back(),
                        None | Some(
                            Component::ParentDir | Component::RootDir | Component::Prefix(_)
                        )
                    ) {
                        path.pop();
                    } else {
                        path.push("..");
                    }
                }
                piece => path.push(piece),
            }
        }
    }
    path
}

#[cfg(test)]
mod tests;
