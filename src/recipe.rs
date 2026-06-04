// src/recipe.rs
// Saved command presets (recipes) for repeated clump operations

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::cli::ClumpParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
}

impl Recipe {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            command: String::new(),
        }
    }

    pub fn to_params(&self) -> Result<ClumpParams> {
        if self.command.trim().is_empty() {
            bail!("Recipe '{}' has no command", self.name);
        }
        let args = shell_split(&self.command);
        if args.is_empty() {
            bail!("Recipe '{}' has an empty command", self.name);
        }
        let full_args: Vec<String> = std::iter::once("clump".to_string()).chain(args).collect();
        let cli: crate::cli::Cli = clap::Parser::try_parse_from(&full_args)
            .map_err(|e| anyhow::anyhow!("Invalid command in recipe '{}': {e}", self.name))?;
        Ok(cli.to_params())
    }
}

pub fn recipes_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("clump")
}

pub fn recipes_path() -> PathBuf {
    recipes_dir().join("recipes.ron")
}

pub fn load_recipes() -> Result<Vec<Recipe>> {
    let path = recipes_path();
    if !path.exists() {
        let defaults = default_recipes();
        save_recipes(&defaults)?;
        return Ok(defaults);
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let recipes: Vec<Recipe> =
        ron::from_str(&contents).with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(recipes)
}

pub fn save_recipes(recipes: &[Recipe]) -> Result<()> {
    let dir = recipes_dir();
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(4)
        .separate_tuple_members(true);
    let s = ron::ser::to_string_pretty(recipes, pretty).context("Failed to serialize recipes")?;
    fs::write(recipes_path(), s).context("Failed to write recipes file")?;
    Ok(())
}

fn default_recipes() -> Vec<Recipe> {
    vec![
        Recipe {
            name: "Rust Project".into(),
            description: "Common Rust project source files".into(),
            command: ". -e target -e .git --only .rs,.toml".into(),
        },
        Recipe {
            name: "Go Project".into(),
            description: "Common Go project source files".into(),
            command: ". -e vendor -e .git --only .go,.mod,.sum".into(),
        },
        Recipe {
            name: "Godot Project".into(),
            description: "Godot project files (scripts, scenes, resources)".into(),
            command: ". -e .godot -e .git --only .gd,.tscn,.tres,.godot,.gdshader".into(),
        },
        Recipe {
            name: "Unity Project".into(),
            description: "Unity project files (C# scripts, shaders, packages)".into(),
            command: ". -e Library -e Temp -e obj -e Build -e Builds -e Logs -e UserSettings --only .cs,.shader,.cginc,.hlsl,.compute,.uss,.uxml,.asset".into(),
        },
        Recipe {
            name: "Unreal Project".into(),
            description: "Unreal Engine project files (C++, Blueprints, assets)".into(),
            command: ". -e Intermediate -e Binaries -e Saved -e DerivedDataCache -e .git --only .h,.cpp,.uproject,.ini,.cs,.usf,.ush".into(),
        },
        Recipe {
            name: "Shallow Snippet".into(),
            description: "Current dir only, no recursion".into(),
            command: ". --shallow".into(),
        },
    ]
}

pub fn shell_split(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if in_double => {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                }
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_split_simple() {
        assert_eq!(shell_split("foo bar baz"), vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_shell_split_quoted() {
        assert_eq!(
            shell_split(r#"foo "bar baz" qux"#),
            vec!["foo", "bar baz", "qux"]
        );
    }

    #[test]
    fn test_shell_split_single_quoted() {
        assert_eq!(
            shell_split("foo 'bar baz' qux"),
            vec!["foo", "bar baz", "qux"]
        );
    }

    #[test]
    fn test_shell_split_escape() {
        assert_eq!(
            shell_split(r#"foo "bar\"baz" qux"#),
            vec!["foo", "bar\"baz", "qux"]
        );
    }
}
