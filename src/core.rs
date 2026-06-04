// src/core.rs
// File collection, filtering, exclusion logic, and output generation engine

use crate::config::SortingMode;
use anyhow::{Context, Result, bail};
use glob::Pattern;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub absolute: PathBuf,
    pub relative: String,
}

impl FileEntry {
    pub fn from_path(path: &Path) -> Result<Self> {
        let absolute = path
            .canonicalize()
            .with_context(|| format!("Failed to resolve path: {}", path.display()))?;
        let relative = path.to_string_lossy().to_string();
        Ok(Self {
            absolute,
            relative: relative.clone(),
        })
    }
}

enum ExcludePattern {
    LiteralPath(String),
    GlobBasename(Pattern),
    GlobPath(Pattern),
}
enum DirExcludePattern {
    LiteralBasename(String),
    LiteralPath(String),
    GlobBasename(Pattern),
    GlobPath(Pattern),
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn classify_exclude(raw: &str) -> ExcludePattern {
    if let Some(rest) = raw.strip_prefix("literal:") {
        return ExcludePattern::LiteralPath(normalize_sep(rest.trim()));
    }
    if has_glob_chars(raw) {
        if raw.contains('/') || raw.contains("**") {
            if let Ok(p) = Pattern::new(raw) {
                return ExcludePattern::GlobPath(p);
            }
        } else {
            if let Ok(p) = Pattern::new(raw) {
                return ExcludePattern::GlobBasename(p);
            }
        }
    }
    ExcludePattern::LiteralPath(normalize_sep(raw))
}

fn classify_dir_exclude(raw: &str) -> DirExcludePattern {
    if let Some(rest) = raw.strip_prefix("literal:") {
        return DirExcludePattern::LiteralBasename(rest.trim().to_string());
    }
    if has_glob_chars(raw) {
        if raw.contains('/') || raw.contains("**") {
            if let Ok(p) = Pattern::new(raw) {
                return DirExcludePattern::GlobPath(p);
            }
        } else {
            if let Ok(p) = Pattern::new(raw) {
                return DirExcludePattern::GlobBasename(p);
            }
        }
    }
    if raw.contains('/') {
        DirExcludePattern::LiteralPath(normalize_sep(raw))
    } else {
        DirExcludePattern::LiteralBasename(raw.to_string())
    }
}

fn normalize_sep(s: &str) -> String {
    s.replace('\\', "/").trim_end_matches('/').to_string()
}
fn compile_dir_patterns(raw: &[String]) -> Vec<DirExcludePattern> {
    raw.iter().map(|r| classify_dir_exclude(r)).collect()
}

pub fn is_binary(path: &Path) -> Result<bool> {
    let file =
        fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut buf = [0u8; 512];
    let n = BufReader::new(file).read(&mut buf)?;
    if n == 0 {
        return Ok(false);
    }
    let buf = &buf[..n];
    if buf.contains(&0) {
        return Ok(true);
    }
    let text_count = buf
        .iter()
        .filter(|&b| {
            b.is_ascii_graphic() || *b == b'\n' || *b == b'\r' || *b == b'\t' || *b == b' '
        })
        .count();
    Ok((text_count as f64) / (n as f64) < 0.8)
}

pub fn collect_files(
    root: &str,
    max_depth: Option<usize>,
    include_hidden: bool,
    exclude_dir_raw: &[String],
) -> Result<Vec<FileEntry>> {
    let root_path =
        fs::canonicalize(root).with_context(|| format!("Failed to resolve path: {root}"))?;
    if !root_path.is_dir() {
        if is_binary(&root_path)? {
            return Ok(Vec::new());
        }
        let rel = root.to_string();
        return Ok(vec![FileEntry {
            absolute: root_path,
            relative: rel,
        }]);
    }

    let dir_patterns = compile_dir_patterns(exclude_dir_raw);
    let max_walk = match max_depth {
        Some(d) => d + 1,
        None => usize::MAX,
    };

    let mut entries = Vec::new();
    for result in walkdir::WalkDir::new(&root_path)
        .max_depth(max_walk)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if !include_hidden && entry.file_name().to_string_lossy().starts_with('.') {
                return false;
            }
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let rel = entry
                    .path()
                    .strip_prefix(&root_path)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                for pat in &dir_patterns {
                    let matched = match pat {
                        DirExcludePattern::LiteralBasename(b) => name == *b,
                        DirExcludePattern::LiteralPath(p) => {
                            rel == *p || rel.starts_with(&format!("{p}/"))
                        }
                        DirExcludePattern::GlobBasename(g) => g.matches(&name),
                        DirExcludePattern::GlobPath(g) => g.matches(&rel),
                    };
                    if matched {
                        return false;
                    }
                }
            }
            true
        })
    {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if is_binary(entry.path()).unwrap_or(true) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(&root_path)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(FileEntry {
            absolute: entry.path().to_path_buf(),
            relative: relative.clone(),
        });
    }

    sort_tree_order(&mut entries, &crate::config::load().sorting);
    Ok(entries)
}

pub fn filter_by_only(entries: &[FileEntry], patterns: &[String]) -> Vec<FileEntry> {
    if patterns.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| {
            let name = e
                .absolute
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let ext = e
                .absolute
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| format!(".{x}"))
                .unwrap_or_default();
            let stem = e
                .absolute
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            patterns.iter().any(|p| {
                if p.starts_with('.') {
                    ext == *p
                } else {
                    name == p || stem == p
                }
            })
        })
        .cloned()
        .collect()
}

pub fn apply_exclusions(
    entries: &[FileEntry],
    exclude_raw: &[String],
    exclude_ext_raw: &[String],
) -> Vec<FileEntry> {
    let compiled: Vec<ExcludePattern> = exclude_raw.iter().map(|r| classify_exclude(r)).collect();
    let exts: Vec<String> = exclude_ext_raw
        .iter()
        .map(|e| {
            if e.starts_with('.') {
                e.clone()
            } else {
                format!(".{e}")
            }
        })
        .collect();
    entries
        .iter()
        .filter(|e| {
            let name = e
                .absolute
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let ext = e
                .absolute
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| format!(".{x}"))
                .unwrap_or_default();
            if exts.iter().any(|ex| *ex == ext) {
                return false;
            }
            for pat in &compiled {
                let matched = match pat {
                    ExcludePattern::LiteralPath(p) => {
                        e.relative == *p || e.relative.starts_with(&format!("{p}/"))
                    }
                    ExcludePattern::GlobBasename(g) => g.matches(name),
                    ExcludePattern::GlobPath(g) => g.matches(&e.relative),
                };
                if matched {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect()
}

pub fn generate_output(
    entries: &[FileEntry],
    header_style: &str,
    separators: bool,
    sep_char: &str,
    sep_placement: &str,
    line_numbers: bool,
    padding: bool,
) -> Result<String> {
    let mut out = String::new();
    for (idx, e) in entries.iter().enumerate() {
        let header = match header_style {
            "absolute" => e.absolute.to_string_lossy().to_string(),
            "none" => String::new(),
            _ => e.relative.clone(),
        };
        let content = fs::read_to_string(&e.absolute)
            .with_context(|| format!("Failed to read {}", e.absolute.display()))?
            .trim_matches('\n')
            .to_string();
        if content.is_empty() {
            continue;
        }

        out.push('\n');
        let above = separators && (sep_placement == "above" || sep_placement == "both");
        let below = separators && (sep_placement == "below" || sep_placement == "both");
        let len = if header.is_empty() { 80 } else { header.len() };

        if above {
            out.push_str(&sep_char.repeat(len));
            out.push('\n');
        }
        if !header.is_empty() {
            out.push_str(&header);
            out.push_str(":\n");
        }
        if below {
            out.push_str(&sep_char.repeat(len));
            out.push('\n');
        }
        out.push('\n');

        if line_numbers {
            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            let width = if padding { total.to_string().len() } else { 0 };
            for (i, line) in lines.iter().enumerate() {
                let num_str = if padding {
                    format!("{:0>width$}", i + 1, width = width)
                } else {
                    (i + 1).to_string()
                };
                out.push_str(&num_str);
                out.push_str(" │ ");
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(&content);
            out.push('\n');
        }

        if idx < entries.len() - 1 {
            out.push('\n');
        }
    }
    out.push('\n');
    Ok(out)
}

pub fn write_to_file(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        eprint!(
            "Output file '{}' already exists. Overwrite? (Y/n): ",
            path.display()
        );
        io::stderr().flush()?;
        let mut r = String::new();
        io::stdin().lock().read_line(&mut r)?;
        if !r.trim().to_lowercase().is_empty() && !r.trim().to_lowercase().starts_with('y') {
            bail!("User cancelled");
        }
    }
    fs::write(path, content).with_context(|| format!("Failed to write to {}", path.display()))?;
    Ok(())
}

pub(crate) fn sort_tree_order(entries: &mut [FileEntry], mode: &SortingMode) {
    match mode {
        SortingMode::Flat => {
            entries.sort_by(|a, b| {
                let name_a = a.relative.rsplit('/').next().unwrap_or("");
                let name_b = b.relative.rsplit('/').next().unwrap_or("");
                name_a.to_lowercase().cmp(&name_b.to_lowercase())
            });
        }
        SortingMode::FilesFirst | SortingMode::DirsFirst => {
            let files_first = matches!(mode, SortingMode::FilesFirst);
            entries.sort_by(|a, b| {
                let pa: Vec<&str> = a.relative.split('/').collect();
                let pb: Vec<&str> = b.relative.split('/').collect();
                let min_len = pa.len().min(pb.len());

                for i in 0..min_len {
                    match pa[i].to_lowercase().cmp(&pb[i].to_lowercase()) {
                        std::cmp::Ordering::Equal => {}
                        ord => return ord,
                    }
                    match pa[i].cmp(pb[i]) {
                        std::cmp::Ordering::Equal => {}
                        ord => {
                            if ord != std::cmp::Ordering::Equal {
                                return ord;
                            }
                        }
                    }

                    let a_ends = i == pa.len() - 1;
                    let b_ends = i == pb.len() - 1;

                    if a_ends != b_ends {
                        if files_first {
                            return if a_ends {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            };
                        } else {
                            return if a_ends {
                                std::cmp::Ordering::Greater
                            } else {
                                std::cmp::Ordering::Less
                            };
                        }
                    }
                }
                pa.len().cmp(&pb.len())
            });
        }
    }
}
