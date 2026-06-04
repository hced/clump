// src/cli.rs
// Command-line argument parsing, dispatch logic, and output handling

use anyhow::{Result, bail};
use chrono::Local;
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use crate::clipboard::copy_to_clipboard;
use crate::config;
use crate::core;
use crate::recipe;

const SAFETY_THRESHOLD: usize = 30;

#[derive(Subcommand, Debug)]
enum Commands {
    Recipe {
        #[command(subcommand)]
        action: Option<RecipeAction>,
    },
}

#[derive(Subcommand, Debug)]
enum RecipeAction {
    List,
    Run { name: String },
    Add { name: String },
    Delete { name: String },
}

#[derive(Parser, Debug)]
#[command(
    name = "clump",
    version,
    propagate_version = true,
    about = "\n\nclump - combine text files into a single output for LLMs, code reviews, or archival.",
    after_help = HELP_TEXT,
)]
pub struct Cli {
    #[arg()]
    pub paths: Vec<String>,

    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    #[arg(short = 'e', long, value_delimiter = ',')]
    pub exclude: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    pub exclude_dir: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    pub exclude_ext: Vec<String>,

    #[arg(
        short = 'L',
        long = "level",
        visible_aliases = ["depth"],
        visible_short_alias = 'd'
    )]
    pub level: Option<usize>,

    #[arg(long = "shallow", conflicts_with = "level")]
    pub shallow: bool,

    #[arg(long, value_enum, default_value_t = HeaderStyle::Relative)]
    pub header_style: HeaderStyle,

    #[arg(long)]
    pub separator: bool,

    #[arg(long)]
    pub include_hidden: bool,

    #[arg(long)]
    pub literal: bool,

    /// Copy output to clipboard (default: true). Use --nocopy to disable.
    #[arg(long, default_value_t = false, action = ArgAction::SetFalse)]
    pub nocopy: bool,

    #[arg(short = 'i', long)]
    pub interactive: bool,

    /// Add line numbers to each file in the output.
    #[arg(long = "ln", visible_short_alias = 'n')]
    pub line_numbers: bool,

    /// Disable zero-padding for line numbers (start at 1 instead of 001).
    #[arg(long = "nopadding")]
    pub no_padding: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
pub enum HeaderStyle {
    #[default]
    Relative,
    Absolute,
    None,
}

impl HeaderStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relative => "relative",
            Self::Absolute => "absolute",
            Self::None => "none",
        }
    }
}

#[derive(Clone)]
pub struct ClumpParams {
    pub search_paths: Vec<String>,
    pub only_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub exclude_exts: Vec<String>,
    pub max_depth: Option<usize>,
    pub include_hidden: bool,
    pub header_style: String,
    pub separator: bool,
    pub output_file: Option<PathBuf>,
    pub nocopy: bool,
    pub selected_files: Option<Vec<PathBuf>>,
    pub line_numbers: bool,
    pub padding: bool,
}

impl Cli {
    pub fn to_params(&self) -> ClumpParams {
        let (detected_exts, actual_paths) = if !self.literal {
            separate_extensions(&self.paths)
        } else {
            (Vec::new(), self.paths.clone())
        };

        let search_paths = if actual_paths.is_empty() {
            vec![".".to_string()]
        } else {
            actual_paths
        };

        let mut only_patterns = flatten_csv(&self.only);
        for ext in &detected_exts {
            let normalized = if ext.starts_with('.') {
                ext.clone()
            } else {
                format!(".{ext}")
            };
            only_patterns.push(normalized);
        }

        let max_depth = if self.shallow { Some(0) } else { self.level };

        ClumpParams {
            search_paths,
            only_patterns,
            exclude_patterns: flatten_csv(&self.exclude),
            exclude_dirs: flatten_csv(&self.exclude_dir),
            exclude_exts: flatten_csv(&self.exclude_ext),
            max_depth,
            include_hidden: self.include_hidden,
            header_style: self.header_style.as_str().to_string(),
            separator: self.separator,
            output_file: self.output.clone(),
            nocopy: self.nocopy,
            selected_files: None,
            line_numbers: self.line_numbers,
            padding: !self.no_padding,
        }
    }
}

pub fn flatten_csv(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for v in values {
        for part in v.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                result.push(trimmed.to_string());
            }
        }
    }
    result
}

pub fn dispatch() -> Result<()> {
    let cli = Cli::parse();

    // If no paths, no command, and no flags -> open TUI
    if cli.paths.is_empty()
        && cli.command.is_none()
        && cli.output.is_none()
        && cli.only.is_empty()
        && cli.exclude.is_empty()
        && cli.exclude_dir.is_empty()
        && cli.exclude_ext.is_empty()
        && cli.level.is_none()
        && !cli.shallow
        && cli.header_style == HeaderStyle::default()
        && !cli.separator
        && !cli.include_hidden
        && !cli.literal
        && !cli.nocopy
        && !cli.interactive
        && !cli.line_numbers
        && !cli.no_padding
    {
        return run_tui();
    }

    match cli.command {
        None => {
            let mut params = cli.to_params();
            if cli.interactive {
                let (p, d, e) = run_interactive()?;
                params.exclude_patterns.extend(p);
                params.exclude_dirs.extend(d);
                params.exclude_exts.extend(e);
            }
            let output = execute_clump(&params, false)?;
            handle_output(&params, &output)
        }
        Some(Commands::Recipe { action }) => match action {
            None => run_tui(),
            Some(RecipeAction::List) => cmd_recipe_list(),
            Some(RecipeAction::Run { name }) => cmd_recipe_run(&name),
            Some(RecipeAction::Add { name }) => cmd_recipe_add(&name),
            Some(RecipeAction::Delete { name }) => cmd_recipe_delete(&name),
        },
    }
}

pub fn execute_clump(params: &ClumpParams, skip_safety: bool) -> Result<String> {
    let cfg = config::load();

    // If specific files were selected via TUI file picker
    if let Some(ref files) = params.selected_files {
        let mut entries: Vec<crate::core::FileEntry> = files
            .iter()
            .filter_map(|p| crate::core::FileEntry::from_path(p).ok())
            .collect();

        core::sort_tree_order(&mut entries, &cfg.sorting);

        return core::generate_output(
            &entries,
            &params.header_style,
            params.separator,
            &cfg.output.separator_char,
            &cfg.output.separator_placement,
            params.line_numbers,
            params.padding,
        );
    }

    let mut all_exclude = cfg.files.exclude_patterns.clone();
    all_exclude.extend(flatten_csv(&params.exclude_patterns));

    let mut all_exclude_dirs = cfg.files.exclude_dirs.clone();
    all_exclude_dirs.extend(flatten_csv(&params.exclude_dirs));

    let mut all_exclude_exts = cfg.extensions.exclude.clone();
    all_exclude_exts.extend(flatten_csv(&params.exclude_exts));

    let max_depth = params.max_depth.or(cfg.files.max_depth);
    let include_hidden = cfg.files.include_hidden || params.include_hidden;

    let mut all_entries = Vec::new();
    for path in &params.search_paths {
        let entries = core::collect_files(path, max_depth, include_hidden, &all_exclude_dirs)?;
        all_entries.extend(entries);
    }

    let all_entries = core::filter_by_only(&all_entries, &params.only_patterns);
    let mut all_entries = core::apply_exclusions(&all_entries, &all_exclude, &all_exclude_exts);
    core::sort_tree_order(&mut all_entries, &cfg.sorting);

    if !skip_safety && all_entries.len() > SAFETY_THRESHOLD && io::stderr().is_terminal() {
        eprintln!(
            "\n  About to combine {} files into a single output.\n  Continue? (y/N): ",
            all_entries.len()
        );
        io::stderr().flush()?;
        let mut response = String::new();
        io::stdin().lock().read_line(&mut response)?;
        if !response.trim().to_lowercase().starts_with('y') {
            bail!("Cancelled");
        }
    }

    core::generate_output(
        &all_entries,
        &params.header_style,
        params.separator || cfg.output.separators,
        &cfg.output.separator_char,
        &cfg.output.separator_placement,
        params.line_numbers,
        params.padding,
    )
}

pub fn handle_output(params: &ClumpParams, output: &str) -> Result<()> {
    // Handle file output first (if specified)
    if let Some(ref path) = params.output_file {
        let final_path = if path.to_string_lossy().ends_with(".txt") || path.extension().is_some() {
            path.clone()
        } else {
            let timestamp = Local::now().format("%Y%m%d%H%M%S");
            path.join(format!("clump_{}.txt", timestamp))
        };

        if final_path.exists() {
            eprint!(
                "File '{}' already exists. Overwrite? (y/N): ",
                final_path.display()
            );
            io::stderr().flush()?;
            let mut response = String::new();
            io::stdin().lock().read_line(&mut response)?;
            if !response.trim().to_lowercase().starts_with('y') {
                bail!("User cancelled file write");
            }
        }

        core::write_to_file(&final_path, output)?;
        eprintln!("Output written to {}", final_path.display());
    }

    // Handle clipboard copy (if not disabled)
    if !params.nocopy {
        check_wayland_clipboard()?;
        copy_to_clipboard(output)?;
        println!("Output copied to clipboard!");
    }

    // Always print to stdout
    print!("{output}");
    Ok(())
}

fn cmd_recipe_list() -> Result<()> {
    let recipes = recipe::load_recipes()?;
    if recipes.is_empty() {
        println!("No recipes found. Create one with: clump recipe add <name>");
    } else {
        println!("Recipes ({}):", recipes.len());
        for r in &recipes {
            let desc = if r.description.is_empty() {
                String::new()
            } else {
                format!(" - {}", r.description)
            };
            println!("  {}{desc}", r.name);
            println!("    {}", r.command);
        }
    }
    Ok(())
}

fn cmd_recipe_run(name: &str) -> Result<()> {
    let recipes = recipe::load_recipes()?;
    let recipe = recipes
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Recipe '{}' not found", name))?;
    let mut params = recipe.to_params()?;
    let cfg = config::load();
    params.include_hidden = cfg.files.include_hidden;
    let output = execute_clump(&params, true)?;
    handle_output(&params, &output)
}

fn cmd_recipe_add(name: &str) -> Result<()> {
    let mut recipes = recipe::load_recipes()?;
    if recipes.iter().any(|r| r.name == name) {
        bail!("Recipe '{}' already exists", name);
    }
    recipes.push(recipe::Recipe::new(name));
    recipe::save_recipes(&recipes)?;
    println!("Created recipe '{}'.", name);
    println!("Edit it with: clump");
    Ok(())
}

fn cmd_recipe_delete(name: &str) -> Result<()> {
    let mut recipes = recipe::load_recipes()?;
    let before = recipes.len();
    recipes.retain(|r| r.name != name);
    if recipes.len() == before {
        bail!("Recipe '{}' not found", name);
    }
    eprint!("Delete recipe '{}'? (y/N): ", name);
    io::stderr().flush()?;
    let mut response = String::new();
    io::stdin().lock().read_line(&mut response)?;
    if !response.trim().to_lowercase().starts_with('y') {
        bail!("Cancelled");
    }
    recipe::save_recipes(&recipes)?;
    println!("Deleted recipe '{}'.", name);
    Ok(())
}

fn run_tui() -> Result<()> {
    #[cfg(feature = "tui")]
    return crate::tui::run();
    #[cfg(not(feature = "tui"))]
    bail!("TUI support not compiled in. Rebuild with --features tui")
}

fn separate_extensions(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut extensions = Vec::new();
    let mut file_paths = Vec::new();
    for p in paths {
        if p.starts_with('.') && !std::path::Path::new(p).exists() {
            extensions.push(p.clone());
        } else {
            file_paths.push(p.clone());
        }
    }
    (extensions, file_paths)
}

fn run_interactive() -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    println!("\n=== Interactive Exclusion ===");
    println!("1. File patterns (e.g., *.tmp, *.log)");
    println!("2. Directories (e.g., test, vendor)");
    println!("3. File extensions (e.g., .tmp, .log)");
    println!("4. Cancel");
    print!("\nChoose (1-4): ");
    io::stdout().flush()?;
    let mut choice = String::new();
    io::stdin().lock().read_line(&mut choice)?;
    let (kind, prompt) = match choice.trim() {
        "1" => ("patterns", "Enter patterns to exclude (space-separated): "),
        "2" => ("dirs", "Enter directories to exclude (space-separated): "),
        "3" => ("exts", "Enter extensions to exclude (space-separated): "),
        _ => return Ok((Vec::new(), Vec::new(), Vec::new())),
    };
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let items: Vec<String> = input
        .trim()
        .split_whitespace()
        .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    print!("\nApply these exclusions? (y/N): ");
    io::stdout().flush()?;
    let mut confirm = String::new();
    io::stdin().lock().read_line(&mut confirm)?;
    if !confirm.trim().to_lowercase().starts_with('y') {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    match kind {
        "patterns" => Ok((items, Vec::new(), Vec::new())),
        "dirs" => Ok((Vec::new(), items, Vec::new())),
        "exts" => Ok((Vec::new(), Vec::new(), items)),
        _ => unreachable!(),
    }
}

fn check_wayland_clipboard() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        if crate::clipboard::is_wayland() {
            let has_wl_copy = crate::clipboard::wl_copy_exists();
            let has_persistent = crate::clipboard::has_persistent_clipboard_daemon();
            if !has_wl_copy && !has_persistent {
                eprintln!(
                    "\n⚠️  Running on Wayland without clipboard support.\nInstall wl-clipboard for basic functionality:\n\x1b[1m  sudo pacman -S wl-clipboard\x1b[0m\n\nFor clipboard content that persists after clump exits,\nalso install a persistent daemon:\n\x1b[1m  • clipvault\x1b[0m  https://github.com/rolv-apneseth/clipvault\n\x1b[1m  • cliphist\x1b[0m   sudo pacman -S cliphist\n\x1b[1m  • cliprust\x1b[0m   https://github.com/aulimaru/cliprust\n\x1b[1m  • stash\x1b[0m      https://github.com/clipcat/stash\n"
                );
                eprint!("Press Enter to continue (clipboard may not work)... ");
                io::stderr().flush()?;
                let mut buf = String::new();
                io::stdin().read_line(&mut buf)?;
                eprintln!();
            } else if has_wl_copy && !has_persistent {
                eprintln!(
                    "\n⚠️  Running on Wayland without a persistent clipboard daemon.\nClipboard content will NOT persist after clump exits.\n\nFor persistent clipboard history, install one of:\n\x1b[1m  • clipvault\x1b[0m  https://github.com/rolv-apneseth/clipvault\n\x1b[1m  • cliphist\x1b[0m   sudo pacman -S cliphist\n\x1b[1m  • cliprust\x1b[0m   https://github.com/aulimaru/cliprust\n\x1b[1m  • stash\x1b[0m      https://github.com/clipcat/stash\n"
                );
                eprint!("Press Enter to continue (clipboard will clear on exit)... ");
                io::stderr().flush()?;
                let mut buf = String::new();
                io::stdin().read_line(&mut buf)?;
                eprintln!();
            }
        }
    }
    Ok(())
}

const HELP_TEXT: &str = r#"
EXCLUSION CHEAT SHEET:
  Want to...                              Use...
  Skip a dir by exact name                -e node_modules
  Skip dirs matching a pattern            --exclude-dir test_*
  Skip a dir only at a specific path      --exclude-dir src/vendor
  Skip a dir at any depth                 --exclude-dir "**/vendor"
  Skip all files of an extension          --exclude-ext .log
  Skip files by basename glob             -e "*.tmp"
  Skip files only under a specific dir    -e "logs/*.log"
  Skip files under any dir of a name      -e "**/fixtures/*"
  Skip one specific file                  -e src/test/data.bin
  Skip an entire tree                     -e build
  File literally named with glob chars    -e "literal:weird*file.txt"

  Multiple flags are merged:
    -e "*.log" -e "tmp/*" -e build

LINE NUMBERING:
  --ln, -n                              Add line numbers to each file in output
  --nopadding                           Disable zero-padding (use 1,2,3 instead of 001,002,003)

  Examples:
    clump . --ln                        # Padded line numbers: 001 │ content
    clump . --ln --nopadding            # Unpadded: 1 │ content

OUTPUT & FORMATTING:
  --header-style <style>                relative (default), absolute, or none
  --separator                           Add separator lines between files
  -o, --output <PATH>                   Write output to file (auto-names if dir given)

EXAMPLES:
  clump                                   # Open TUI (default)
  clump .                                 # CLI: current directory (recursive)
  clump . --shallow                       # CLI: current dir only, no recursion
  clump .go .toml                         # CLI: Only .go and .toml (smart)
  clump . --literal .go                   # CLI: File literally named ".go"
  clump file1.txt file2.md                # CLI: Specific files
  clump src/                              # CLI: All text files in src/
  clump . -o output.txt                   # CLI: Save to file
  clump . -o ./results/                   # CLI: Auto-name: results/clump_TIMESTAMP.txt
  clump . --nocopy                        # CLI: Print only, no clipboard
  clump . --only .go,.md                  # CLI: Only Go and Markdown
  clump . -e "*.log" -e "tmp/*"           # CLI: Exclude by pattern
  clump . --exclude-dir node_modules,.git # CLI: Skip entire directories
  clump . --exclude-dir test_*            # CLI: Skip dirs matching pattern
  clump . --exclude-ext .tmp,.log,json    # CLI: Exclude by extension
  clump . -e build                        # CLI: Exclude entire build tree
  clump . -L 2                            # CLI: Max depth 2
  clump . --header-style absolute         # CLI: Absolute paths in headers
  clump . --separator                     # CLI: Enable separators
  clump . --ln                            # CLI: Enable line numbers
  clump . --ln --nopadding                # CLI: Line numbers without padding

RECIPES:
  clump recipe                            # Open recipe manager in TUI
  clump recipe list                       # List all recipes
  clump recipe run <name>                 # Run a recipe
  clump recipe add <name>                 # Create empty recipe
  clump recipe delete <name>              # Delete a recipe

CONFIGURATION:
  Config file: ~/.config/clump/config.ron
  Settings: sorting mode, hidden files, exclusions, output format

TUI KEYBINDINGS:
  Main Menu:
    ↑↓/jk: Navigate  Enter/Space: Select  Esc/Q: Quit

  File Picker:
    ↑↓/jk: Navigate files  ←h/→l: Navigate dirs
    Space: Toggle selection  Enter: Confirm  Esc: Cancel
    .: Toggle hidden files  Tab/f: Toggle fuzzy search
    In fuzzy mode: ↑↓: Navigate  Ctrl+←/→: Dir nav  Type to filter

  Recipe Manager:
    ↑↓/jk: Navigate  r: Run  e: Edit  a: Add  d: Delete
    Tab/f: Toggle fuzzy search  Esc: Back
    In edit mode: ↑↓: Switch field  ←→: Cursor  Enter: Save  Esc: Cancel

  Output Options:
    ↑↓/jk: Navigate  Enter: Choose output method
    l: Toggle line numbers  Esc: Back

  Configuration:
    ↑↓/jk: Navigate  Enter: Toggle setting  Esc: Back

WAYLAND CLIPBOARD:
  If running on Wayland without wl-clipboard, install it:
    sudo pacman -S wl-clipboard

  For persistent clipboard history, also install one of:
    • clipvault  https://github.com/rolv-apneseth/clipvault
    • cliphist   sudo pacman -S cliphist
    • cliprust   https://github.com/aulimaru/cliprust
    • stash      https://github.com/clipcat/stash
"#;
