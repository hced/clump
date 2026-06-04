// src/config.rs
// Persistent configuration management (sorting mode, hidden files) via RON files

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SortingMode {
    #[serde(rename = "files_first")]
    FilesFirst,
    #[serde(rename = "dirs_first")]
    DirsFirst,
    #[serde(rename = "flat")]
    Flat,
}

impl Default for SortingMode {
    fn default() -> Self {
        Self::FilesFirst
    }
}

// New: Header alignment options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum HeaderAlignment {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "center")]
    #[default]
    Center,
    #[serde(rename = "right")]
    Right,
}

impl From<HeaderAlignment> for ratatui::prelude::Alignment {
    fn from(al: HeaderAlignment) -> Self {
        match al {
            HeaderAlignment::Left => ratatui::prelude::Alignment::Left,
            HeaderAlignment::Center => ratatui::prelude::Alignment::Center,
            HeaderAlignment::Right => ratatui::prelude::Alignment::Right,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub sorting: SortingMode,
    #[serde(default = "default_files_config")]
    pub files: FilesConfig,
    #[serde(default)]
    pub extensions: ExtensionsConfig,
    #[serde(default = "default_output_config")]
    pub output: OutputConfig,
    #[serde(default)]
    pub preview: PreviewConfig,
    // New: UI configuration
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FilesConfig {
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(
        default,
        deserialize_with = "deserialize_max_depth",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExtensionsConfig {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutputConfig {
    #[serde(default = "default_str")]
    pub header_style: String,
    #[serde(default)]
    pub separators: bool,
    #[serde(default = "default_char")]
    pub separator_char: String,
    #[serde(default = "default_placement")]
    pub separator_placement: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct PreviewConfig {
    #[serde(default = "default_usize")]
    pub max_lines: usize,
    #[serde(default)]
    pub binary_warning: bool,
}

// New: UI configuration struct
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UiConfig {
    #[serde(default)]
    pub header_alignment: HeaderAlignment,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            header_alignment: HeaderAlignment::default(),
        }
    }
}

fn default_str() -> String {
    "relative".into()
}
fn default_char() -> String {
    "-".into()
}
fn default_placement() -> String {
    "both".into()
}
fn default_usize() -> usize {
    10000
}

fn default_files_config() -> FilesConfig {
    FilesConfig {
        include_hidden: false,
        max_depth: None,
        exclude_dirs: vec!["node_modules".into(), ".git".into()],
        exclude_patterns: Vec::new(),
    }
}

fn default_output_config() -> OutputConfig {
    OutputConfig {
        header_style: default_str(),
        separators: true,
        separator_char: default_char(),
        separator_placement: default_placement(),
    }
}

fn deserialize_max_depth<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    struct V;
    impl<'de> de::Visitor<'de> for V {
        type Value = Option<usize>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an integer or null")
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_u64(Self)
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(if v == 0 { None } else { Some(v as usize) })
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(if v <= 0 { None } else { Some(v as usize) })
        }
    }
    deserializer.deserialize_option(V)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sorting: SortingMode::default(),
            files: default_files_config(),
            extensions: ExtensionsConfig::default(),
            output: default_output_config(),
            preview: PreviewConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clump/config.ron")
}

pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        return Config::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| ron::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config) -> Result<(), std::io::Error> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(4)
        .separate_tuple_members(true);
    let s = ron::ser::to_string_pretty(cfg, pretty).unwrap();
    fs::write(&path, s)
}
