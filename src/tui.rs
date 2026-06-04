// src/tui.rs
// Terminal UI state machine, rendering, and event routing for all interactive screens

use anyhow::Result;
use chrono::Local;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use incredimo::Banner;
use ratatui::{
    prelude::*,
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{
    io::{self, Write},
    path::PathBuf,
};

use crate::cli::{self, ClumpParams};
use crate::clipboard::copy_to_clipboard;
use crate::config::{self, Config, SortingMode};
use crate::core::write_to_file;
use crate::filepicker::{FilePicker, PickerAction};
use crate::fuzzypicker::FuzzySearch;
use crate::recipe::{self, Recipe};

#[derive(Clone, Copy, PartialEq)]
enum TuiState {
    MainMenu,
    FilePicker,
    RecipesMenu,
    RecipeAction,
    OutputOptions,
    ConfigEditor,
    Help,
}

#[derive(Clone, Copy, PartialEq)]
enum RecipeMode {
    Browse,
    Edit,
    Fuzzy,
}

pub struct RecipeForm {
    fields: [String; 3],
    cursors: [usize; 3],
    active_field: usize,
}

impl RecipeForm {
    fn new() -> Self {
        Self {
            fields: [String::new(), String::new(), String::new()],
            cursors: [0, 0, 0],
            active_field: 0,
        }
    }
    fn clear(&mut self) {
        self.fields = [String::new(), String::new(), String::new()];
        self.cursors = [0, 0, 0];
        self.active_field = 0;
    }

    fn load(&mut self, name: &str, desc: &str, cmd: &str) {
        self.fields = [name.to_string(), desc.to_string(), cmd.to_string()];
        self.cursors = [name.len(), desc.len(), cmd.len()];
        self.active_field = 0;
    }

    fn step_cursor(&mut self, dir: i32) {
        let max = self.fields[self.active_field].chars().count();
        let cur = self.cursors[self.active_field];
        self.cursors[self.active_field] = if dir > 0 {
            (cur + 1).min(max)
        } else {
            cur.saturating_sub(1)
        };
    }
    fn insert_char(&mut self, c: char) {
        let idx = self.active_field;
        let mut t: Vec<char> = self.fields[idx].chars().collect();
        t.insert(self.cursors[idx], c);
        self.fields[idx] = t.into_iter().collect();
        self.cursors[idx] += 1;
    }
    fn delete_before_cursor(&mut self) {
        let idx = self.active_field;
        if self.cursors[idx] > 0 {
            let mut t: Vec<char> = self.fields[idx].chars().collect();
            t.remove(self.cursors[idx] - 1);
            self.fields[idx] = t.into_iter().collect();
            self.cursors[idx] -= 1;
        }
    }
    fn delete_at_cursor(&mut self) {
        let idx = self.active_field;
        if self.cursors[idx] < self.fields[idx].chars().count() {
            let mut t: Vec<char> = self.fields[idx].chars().collect();
            t.remove(self.cursors[idx]);
            self.fields[idx] = t.into_iter().collect();
        }
    }
}

pub struct TuiApp {
    state: TuiState,
    recipes: Vec<Recipe>,
    output_text: Option<String>,
    main_menu_state: ListState,
    recipes_menu_state: ListState,
    recipe_action_state: ListState,
    output_menu_state: ListState,
    config_menu_state: ListState,
    file_picker: FilePicker,
    banner_text: Text<'static>,
    banner_height: u16,
    pending_print: Option<String>,
    app_config: Config,
    recipe_search: FuzzySearch,
    recipe_mode: RecipeMode,
    recipe_form: RecipeForm,
    recipe_edit_target: Option<usize>,
    active_params: Option<ClumpParams>,
    line_numbers: bool,
    // Help screen state
    help_lines: Vec<Line<'static>>,
    help_offset: usize,
}

impl TuiApp {
    pub fn new() -> Result<Self> {
        let recipes = recipe::load_recipes().unwrap_or_else(|e| {
            eprintln!("Warning: failed to load recipes: {e}");
            Vec::new()
        });
        let app_config = config::load();
        let root = std::env::current_dir()?;
        let file_picker = FilePicker::new(&root)?;
        let banner = Banner::new("clump")
            .with_subtitle(&format!("v{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build banner");
        let raw_banner = strip_ansi(&banner.render());

        // Banner with breathing room
        let mut lines: Vec<Line<'static>> = vec![Line::raw("")];
        lines.extend(
            raw_banner.lines().map(|l| {
                Line::styled(l.to_string(), Style::default().fg(Color::Rgb(180, 140, 90)))
            }),
        );

        let banner_height = lines.len().min(9) as u16;

        // Help text construction
        let help_text = r#"CLUMP - Concatenate Files for Collaboration & Reference

PURPOSE:
Clump was designed to streamline the process of combining entire codebases
or project directories into a single, flat document. This is incredibly
useful for:
• Sharing project context with AI assistants for collaborative coding
• Providing reviewers with a complete snapshot of your codebase
• Archiving project states as readable text files
• Quick reference when jumping between complex repositories

HOW IT WORKS:
Clump intelligently traverses your directory, filters out binary files
and noise (like node_modules or .git), and stitches together all text
files into one clean output.

USAGE MODES:
1. CLI Mode:
   Run `clump` with paths and flags to process files directly in the
   terminal. Perfect for scripting and quick exports.

   Example: `clump src/ --ln -o project_dump.txt`

2. TUI Mode (Default):
   Running `clump` without arguments opens an interactive terminal UI.
   Navigate with arrow keys, search with fuzzy matching, and select
   specific files or directories.

3. Recipes:
   Save common clump configurations as "Recipes" to quickly reuse
   filters, paths, and settings for recurring projects.

FEATURES:
• Fuzzy File Search: Instantly filter files by name.
• Line Numbering: Optional per-file line numbers with padding.
• Recipes: Save and manage complex filtering profiles.
• Smart Exclusions: Automatically ignores binary files and common
  directories, with customizable patterns.
• Clipboard Integration: One-click copy to system clipboard.

TUI CONTROLS:
Main Menu:
  ↑↓/jk: Navigate  Enter/Space: Select  Esc/Q: Quit

File Picker:
  ↑↓/jk: Navigate  ←h/→l: Change Dir  Space: Toggle Selection
  Tab/f: Fuzzy Find  .: Toggle Hidden  Enter: Confirm

Recipes:
  ↑↓/jk: Navigate  r: Run  e: Edit  a: Add  d: Delete
  Tab/f: Fuzzy Find  Esc: Back

Output Options:
  Choose clipboard, file, or terminal output.
  Press 'l' to toggle line numbers before choosing.

CONFIGURATION:
  Located at: ~/.config/clump/config.ron
  Customize sorting, hidden files, and UI preferences here.

FOR MORE INFO:
  Run `clump --help` for CLI flags and examples."#;

        let help_lines: Vec<Line<'static>> = help_text
            .lines()
            .map(|l| Line::styled(l.to_string(), Style::default().fg(Color::White)))
            .collect();

        let mut app = Self {
            state: TuiState::MainMenu,
            recipes,
            output_text: None,
            main_menu_state: ListState::default(),
            recipes_menu_state: ListState::default(),
            recipe_action_state: ListState::default(),
            output_menu_state: ListState::default(),
            config_menu_state: ListState::default(),
            file_picker,
            banner_text: Text::from(lines),
            banner_height,
            pending_print: None,
            app_config,
            recipe_search: FuzzySearch::new(),
            recipe_mode: RecipeMode::Browse,
            recipe_form: RecipeForm::new(),
            recipe_edit_target: None,
            active_params: None,
            line_numbers: false,
            help_lines,
            help_offset: 0,
        };
        app.refresh_recipe_matches();
        Ok(app)
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        if self.main_menu_state.selected().is_none() {
            self.main_menu_state.select(Some(0));
        }
        if self.recipes_menu_state.selected().is_none() {
            self.recipes_menu_state.select(Some(0));
        }
        if self.recipe_action_state.selected().is_none() {
            self.recipe_action_state.select(Some(0));
        }
        if self.output_menu_state.selected().is_none() {
            self.output_menu_state.select(Some(0));
        }
        if self.config_menu_state.selected().is_none() {
            self.config_menu_state.select(Some(0));
        }

        loop {
            terminal.draw(|f| self.ui(f))?;
            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key)? {
                        break;
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        config::save(&self.app_config)
            .unwrap_or_else(|e| eprintln!("Warning: failed to save config: {}", e));

        if let Some(content) = self.pending_print.take() {
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(content.as_bytes());
            let _ = stdout.flush();
        }
        Ok(())
    }

    fn handle_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        match self.state {
            TuiState::MainMenu => self.handle_main_menu_key(key),
            TuiState::FilePicker => self.handle_file_picker_key(key),
            TuiState::RecipesMenu => self.handle_recipes_menu_key(key),
            TuiState::RecipeAction => self.handle_recipe_action_key(key),
            TuiState::OutputOptions => self.handle_output_options_key(key),
            TuiState::ConfigEditor => self.handle_config_key(key),
            TuiState::Help => self.handle_help_key(key),
        }
    }

    fn handle_help_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }
        let max_offset = self.help_lines.len().saturating_sub(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.help_offset > 0 {
                    self.help_offset -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.help_offset < max_offset {
                    self.help_offset += 1;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = TuiState::MainMenu;
                self.main_menu_state.select(Some(0));
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_main_menu_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        let items = self.get_main_menu_items();
        let len = items.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if len > 0 {
                    let i = self.main_menu_state.selected().unwrap_or(0);
                    self.main_menu_state.select(Some((i + 1) % len));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if len > 0 {
                    let i = self.main_menu_state.selected().unwrap_or(0);
                    self.main_menu_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(i) = self.main_menu_state.selected() {
                    if let Some((id, _)) = items.get(i) {
                        match id.as_str() {
                            "select_files" => {
                                self.state = TuiState::FilePicker;
                                self.file_picker.selected.clear();
                                self.file_picker.fuzzy_active = false;
                                self.file_picker.search.pattern.clear();
                                self.file_picker.current_dir = self.file_picker.root.clone();
                                self.file_picker.load_entries()?;
                                return Ok(false);
                            }
                            "recipes" => {
                                self.state = TuiState::RecipesMenu;
                                self.recipe_mode = RecipeMode::Browse;
                                self.sync_form_to_selected();
                                self.refresh_recipe_matches();
                                self.recipes_menu_state.select(Some(0));
                            }
                            "config" => {
                                self.state = TuiState::ConfigEditor;
                                self.config_menu_state.select(Some(0));
                            }
                            "help" => {
                                self.state = TuiState::Help;
                                self.help_offset = 0;
                            }
                            "quit" => return Ok(true),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn handle_file_picker_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        match self.file_picker.handle_event(&key) {
            PickerAction::Confirmed(files) => {
                if !files.is_empty() {
                    let params = ClumpParams {
                        search_paths: vec![],
                        only_patterns: vec![],
                        exclude_patterns: vec![],
                        exclude_dirs: vec![],
                        exclude_exts: vec![],
                        max_depth: None,
                        include_hidden: self.app_config.files.include_hidden,
                        header_style: "relative".to_string(),
                        separator: true,
                        output_file: None,
                        nocopy: false,
                        selected_files: Some(files),
                        line_numbers: self.line_numbers,
                        padding: true,
                    };
                    self.active_params = Some(params.clone());
                    self.output_text = Some(cli::execute_clump(&params, true)?);
                    self.state = TuiState::OutputOptions;
                    self.output_menu_state.select(Some(0));
                } else {
                    self.state = TuiState::MainMenu;
                }
                Ok(false)
            }
            PickerAction::Cancelled => {
                self.state = TuiState::MainMenu;
                Ok(false)
            }
            PickerAction::Continue => Ok(false),
        }
    }

    fn sync_form_to_selected(&mut self) {
        let idx = self.recipes_menu_state.selected().unwrap_or(0);
        let data = self
            .get_visible_recipes()
            .get(idx)
            .map(|r| (r.name.clone(), r.description.clone(), r.command.clone()));

        if let Some((name, desc, cmd)) = data {
            self.recipe_form.load(&name, &desc, &cmd);
        }
    }

    fn save_current_recipe_form(&mut self) {
        if let Some(idx) = self.recipe_edit_target {
            if let Some(r) = self.recipes.get_mut(idx) {
                r.name = self.recipe_form.fields[0].clone();
                r.description = self.recipe_form.fields[1].clone();
                r.command = self.recipe_form.fields[2].clone();
            }
        } else {
            self.recipes.push(Recipe {
                name: self.recipe_form.fields[0].clone(),
                description: self.recipe_form.fields[1].clone(),
                command: self.recipe_form.fields[2].clone(),
            });
        }
        recipe::save_recipes(&self.recipes).unwrap_or_else(|e| eprintln!("Warning: {}", e));
        self.refresh_recipe_matches();
        self.sync_form_to_selected();
    }

    fn handle_recipes_menu_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        if self.recipe_mode == RecipeMode::Browse
            && (key.code == KeyCode::Char('f')
                || key.code == KeyCode::Char('F')
                || key.code == KeyCode::Tab)
        {
            self.recipe_mode = RecipeMode::Fuzzy;
            self.recipe_search.pattern.clear();
            self.recipe_search.cursor = 0;
            self.recipes_menu_state.select(Some(0));
            return Ok(false);
        }

        if self.recipe_mode == RecipeMode::Fuzzy
            && (key.code == KeyCode::Esc
                || key.code == KeyCode::Tab
                || key.code == KeyCode::Char('f'))
        {
            self.recipe_mode = RecipeMode::Browse;
            self.recipe_search.pattern.clear();
            self.refresh_recipe_matches();
            self.recipes_menu_state.select(Some(0));
            return Ok(false);
        }

        if self.recipe_mode == RecipeMode::Edit && key.code == KeyCode::Esc {
            self.recipe_mode = RecipeMode::Browse;
            self.sync_form_to_selected();
            return Ok(false);
        }

        match self.recipe_mode {
            RecipeMode::Browse => {
                let len = self.get_visible_recipes().len();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if len > 0 {
                            let i = self.recipes_menu_state.selected().unwrap_or(0);
                            self.recipes_menu_state.select(Some(if i == 0 {
                                len - 1
                            } else {
                                i - 1
                            }));
                        }
                        self.sync_form_to_selected();
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if len > 0 {
                            let i = self.recipes_menu_state.selected().unwrap_or(0);
                            self.recipes_menu_state.select(Some((i + 1) % len));
                        }
                        self.sync_form_to_selected();
                    }
                    KeyCode::Enter | KeyCode::Char('r') => {
                        if let Some(i) = self.recipes_menu_state.selected() {
                            if let Some(recipe) =
                                self.get_visible_recipes().get(i).map(|r| r.clone())
                            {
                                if let Ok(mut params) = recipe.to_params() {
                                    params.include_hidden = self.app_config.files.include_hidden;
                                    params.line_numbers = self.line_numbers;
                                    params.padding = true;
                                    self.active_params = Some(params.clone());
                                    if let Ok(output) = cli::execute_clump(&params, true) {
                                        self.output_text = Some(output);
                                        self.state = TuiState::OutputOptions;
                                        self.output_menu_state.select(Some(0));
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(i) = self.recipes_menu_state.selected() {
                            let original_idx = i;
                            self.sync_form_to_selected();
                            self.recipe_edit_target = Some(original_idx);
                            self.recipe_mode = RecipeMode::Edit;
                        }
                    }
                    KeyCode::Char('a') => {
                        self.recipe_form.clear();
                        self.recipe_edit_target = None;
                        self.recipe_mode = RecipeMode::Edit;
                    }
                    KeyCode::Char('d') => {
                        if let Some(i) = self.recipes_menu_state.selected() {
                            if let Some(name) =
                                self.get_visible_recipes().get(i).map(|r| r.name.clone())
                            {
                                self.recipes.retain(|r| r.name != name);
                                recipe::save_recipes(&self.recipes)
                                    .unwrap_or_else(|e| eprintln!("Warning: {}", e));
                                self.refresh_recipe_matches();
                                self.sync_form_to_selected();
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.state = TuiState::MainMenu;
                        self.main_menu_state.select(Some(0));
                    }
                    _ => {}
                }
            }
            RecipeMode::Edit => match key.code {
                KeyCode::Up => {
                    self.recipe_form.active_field = if self.recipe_form.active_field == 0 {
                        2
                    } else {
                        self.recipe_form.active_field - 1
                    };
                }
                KeyCode::Down => {
                    self.recipe_form.active_field = (self.recipe_form.active_field + 1) % 3;
                }
                KeyCode::Left => {
                    self.recipe_form.step_cursor(-1);
                }
                KeyCode::Right => {
                    self.recipe_form.step_cursor(1);
                }
                KeyCode::Backspace => {
                    self.recipe_form.delete_before_cursor();
                }
                KeyCode::Delete => {
                    self.recipe_form.delete_at_cursor();
                }
                KeyCode::Char(c) if !c.is_control() => {
                    self.recipe_form.insert_char(c);
                }
                KeyCode::Enter => {
                    self.save_current_recipe_form();
                    self.recipe_mode = RecipeMode::Browse;
                }
                _ => {}
            },
            RecipeMode::Fuzzy => match key.code {
                KeyCode::Backspace => {
                    self.recipe_search.backspace();
                    self.refresh_recipe_matches();
                }
                KeyCode::Delete => {
                    self.recipe_search.delete();
                    self.refresh_recipe_matches();
                }
                KeyCode::Left if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recipe_search.move_cursor_left();
                }
                KeyCode::Right if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recipe_search.move_cursor_right();
                }
                KeyCode::Char(c) if !c.is_control() => {
                    self.recipe_search.handle_input(c);
                    self.refresh_recipe_matches();
                }

                KeyCode::Up | KeyCode::Char('k') => {
                    let len = self.get_visible_recipes().len();
                    if len > 0 {
                        let i = self.recipes_menu_state.selected().unwrap_or(0);
                        self.recipes_menu_state
                            .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                    }
                    self.sync_form_to_selected();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = self.get_visible_recipes().len();
                    if len > 0 {
                        let i = self.recipes_menu_state.selected().unwrap_or(0);
                        self.recipes_menu_state.select(Some((i + 1) % len));
                    }
                    self.sync_form_to_selected();
                }
                KeyCode::Enter | KeyCode::Char('r') => {
                    if let Some(i) = self.recipes_menu_state.selected() {
                        if let Some(recipe) = self.get_visible_recipes().get(i).map(|r| r.clone()) {
                            if let Ok(mut params) = recipe.to_params() {
                                params.include_hidden = self.app_config.files.include_hidden;
                                params.line_numbers = self.line_numbers;
                                params.padding = true;
                                self.active_params = Some(params.clone());
                                if let Ok(output) = cli::execute_clump(&params, true) {
                                    self.output_text = Some(output);
                                    self.state = TuiState::OutputOptions;
                                    self.output_menu_state.select(Some(0));
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
        }
        Ok(false)
    }

    fn handle_recipe_action_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        if self.recipes.is_empty() {
            if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
                self.state = TuiState::RecipesMenu;
                return Ok(false);
            }
            return Ok(false);
        }
        let items: Vec<&str> = self.recipes.iter().map(|r| r.name.as_str()).collect();
        let len = items.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.recipe_action_state.selected().unwrap_or(0);
                self.recipe_action_state.select(Some((i + 1) % len));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.recipe_action_state.selected().unwrap_or(0);
                self.recipe_action_state
                    .select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(i) = self.recipe_action_state.selected() {
                    if let Some(name) = items.get(i) {
                        if let Some(r) = self.recipes.iter().find(|r| r.name == *name).cloned() {
                            let mut params = r.to_params()?;
                            params.include_hidden = self.app_config.files.include_hidden;
                            self.output_text = Some(cli::execute_clump(&params, true)?);
                            self.state = TuiState::OutputOptions;
                            self.output_menu_state.select(Some(0));
                        }
                    }
                }
            }
            KeyCode::Esc => {
                self.state = TuiState::RecipesMenu;
                self.recipes_menu_state.select(Some(0));
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_output_options_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        let len = 5;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.output_menu_state.selected().unwrap_or(0);
                self.output_menu_state.select(Some((i + 1) % len));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.output_menu_state.selected().unwrap_or(0);
                self.output_menu_state
                    .select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.line_numbers = !self.line_numbers;
                if let Some(ref p) = self.active_params {
                    let mut p = p.clone();
                    p.line_numbers = self.line_numbers;
                    p.padding = true;
                    if let Ok(out) = cli::execute_clump(&p, true) {
                        self.output_text = Some(out);
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(i) = self.output_menu_state.selected() {
                    match i {
                        0 => {
                            if let Some(ref o) = self.output_text {
                                if let Err(e) = copy_to_clipboard(o) {
                                    eprintln!("⚠️  {}", e);
                                }
                            }
                            return Ok(true);
                        }
                        // Index 1: Print To Terminal (No Line Numbers)
                        1 => {
                            self.pending_print = self.output_text.clone();
                            return Ok(true);
                        }
                        // Index 2: Print To Terminal (+ Line Numbers)
                        2 => {
                            if let Some(ref p) = self.active_params {
                                let mut p = p.clone();
                                p.line_numbers = true;
                                p.padding = true;
                                if let Ok(out) = cli::execute_clump(&p, true) {
                                    self.pending_print = Some(out);
                                    return Ok(true);
                                }
                            }
                        }
                        // Index 3: Save To File
                        3 => {
                            if let Some(ref o) = self.output_text {
                                // Generate timestamped filename: clump_YYYYMMDDHHMMSS.txt
                                let timestamp = Local::now().format("%Y%m%d%H%M%S");
                                let filename = format!("clump_{}.txt", timestamp);
                                write_to_file(&PathBuf::from(filename), o)?;
                            }
                            return Ok(true);
                        }
                        // Index 4: Back
                        4 => {
                            self.state = TuiState::MainMenu;
                            self.main_menu_state.select(Some(0));
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
            }
            KeyCode::Esc => {
                self.state = TuiState::MainMenu;
                self.main_menu_state.select(Some(0));
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_config_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        let len = 3;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.config_menu_state.selected().unwrap_or(0);
                self.config_menu_state.select(Some((i + 1) % len));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.config_menu_state.selected().unwrap_or(0);
                self.config_menu_state
                    .select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                match self.config_menu_state.selected().unwrap_or(0) {
                    0 => {
                        self.app_config.sorting = match &self.app_config.sorting {
                            SortingMode::FilesFirst => SortingMode::DirsFirst,
                            SortingMode::DirsFirst => SortingMode::Flat,
                            _ => SortingMode::FilesFirst,
                        }
                    }
                    1 => {
                        self.app_config.files.include_hidden = !self.app_config.files.include_hidden
                    }
                    2 => {
                        self.state = TuiState::MainMenu;
                        self.main_menu_state.select(Some(0));
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => {
                self.state = TuiState::MainMenu;
                self.main_menu_state.select(Some(0));
            }
            _ => {}
        }
        Ok(false)
    }

    fn get_visible_recipes(&self) -> Vec<&Recipe> {
        if self.recipe_mode == RecipeMode::Fuzzy && !self.recipe_search.pattern.is_empty() {
            self.recipe_search
                .matches
                .iter()
                .filter_map(|&i| self.recipes.get(i))
                .collect()
        } else {
            self.recipes.iter().collect()
        }
    }

    fn refresh_recipe_matches(&mut self) {
        if self.recipe_search.pattern.is_empty() {
            self.recipe_search.matches = (0..self.recipes.len()).collect();
            return;
        }
        let haystacks: Vec<String> = self
            .recipes
            .iter()
            .map(|r| format!("{} {}", r.name, r.description))
            .collect();
        let refs: Vec<&str> = haystacks.iter().map(|s| s.as_str()).collect();
        self.recipe_search.update_matches(&refs);
    }

    fn get_main_menu_items(&self) -> Vec<(String, String)> {
        vec![
            ("select_files".to_string(), "📁 Select".to_string()),
            ("recipes".to_string(), "📚 Recipes".to_string()),
            ("config".to_string(), "⚙️ Configuration".to_string()),
            ("help".to_string(), "❓ Help".to_string()),
            ("quit".to_string(), "Quit".to_string()),
        ]
    }

    fn footer_text(&self) -> String {
        match self.state {
            TuiState::MainMenu => "↑↓/jk: Navigate  Enter/Space: Select  Esc/Q: Quit".into(),
            TuiState::FilePicker => {
                let fp = &self.file_picker;
                let mode_text = if fp.fuzzy_active {
                    "Tab/Esc: Exit Find"
                } else {
                    "Tab/f: Find"
                };
                let nav_text = if fp.fuzzy_active {
                    "↑↓: Nav  Ctrl+←/→: Dir"
                } else {
                    "↑↓/jk: Nav  ←h/→l: Dir"
                };
                let period_text = if fp.show_hidden {
                    ".: Hide hidden"
                } else {
                    ".: Show hidden"
                };
                format!(
                    "{}  Space: Toggle  Ctrl+.  Enter: Confirm  {}  {}",
                    nav_text, period_text, mode_text
                )
            }
            TuiState::RecipesMenu => match self.recipe_mode {
                RecipeMode::Browse => {
                    "↑↓/jk: Nav  r: Run  e: Edit  a: Add  d: Del  f/Tab: Find  Esc: Back"
                }
                RecipeMode::Edit => "↑↓: Switch Field  ←→: Cursor  Enter: Save  Esc: Cancel",
                RecipeMode::Fuzzy => "↑↓/jk: Nav  r: Run  ←→: Cursor  Esc/f: Exit Find",
            }
            .into(),
            TuiState::RecipeAction => "↑↓/jk: Select  Enter/Space: Run  Esc: Back".into(),
            TuiState::OutputOptions => {
                let ln = if self.line_numbers { "ON" } else { "OFF" };
                format!(
                    "↑↓/jk: Navigate  Enter/Space: Choose  l: Line# {}  Esc: Back",
                    ln
                )
            }
            TuiState::ConfigEditor => "↑↓/jk: Navigate  Enter: Toggle  Esc: Back".into(),
            TuiState::Help => "↑↓/jk: Scroll  Esc: Back".into(),
        }
    }

    fn ui(&self, f: &mut Frame) {
        let area = f.area();
        let header_h = self.banner_height;
        let footer_h = 1;
        let content_y = header_h;
        let content_h = area.height.saturating_sub(header_h + footer_h);
        let footer_y = area.height.saturating_sub(footer_h);

        f.render_widget(
            Paragraph::new(self.banner_text.clone()).alignment(Alignment::Center),
            Rect {
                x: 0,
                y: 0,
                width: area.width,
                height: header_h,
            },
        );

        let (list_area, form_area) = if self.state == TuiState::RecipesMenu {
            let form_h = (content_h * 2 / 5).max(7);
            let list_h = content_h - form_h;
            (
                Rect {
                    x: 0,
                    y: content_y,
                    width: area.width,
                    height: list_h,
                },
                Rect {
                    x: 0,
                    y: content_y + list_h,
                    width: area.width,
                    height: form_h,
                },
            )
        } else {
            (
                Rect {
                    x: 0,
                    y: content_y,
                    width: area.width,
                    height: content_h,
                },
                Rect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
            )
        };

        match self.state {
            TuiState::MainMenu => self.render_main_menu(f, list_area),
            TuiState::FilePicker => self.file_picker.draw(f, list_area),
            TuiState::RecipesMenu => {
                self.render_recipes_menu(f, list_area);
                self.render_recipe_form(f, form_area);
            }
            TuiState::RecipeAction => self.render_recipe_action(f, list_area),
            TuiState::OutputOptions => self.render_output_options(f, list_area),
            TuiState::ConfigEditor => self.render_config_editor(f, list_area),
            TuiState::Help => self.render_help(f, list_area),
        }

        f.render_widget(
            Paragraph::new(self.footer_text())
                .style(Style::default().fg(Color::Gray))
                .alignment(Alignment::Center),
            Rect {
                x: 0,
                y: footer_y,
                width: area.width,
                height: footer_h,
            },
        );
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Help & Usage ")
            .borders(Borders::ALL);
        let paragraph = Paragraph::new(Text::from(self.help_lines.clone()))
            .block(block)
            .scroll((self.help_offset as u16, 0));
        f.render_widget(paragraph, area);
    }

    fn render_recipes_menu(&self, f: &mut Frame, mut area: Rect) {
        if self.recipe_mode == RecipeMode::Fuzzy {
            let search_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 3,
            };
            let search_block = Block::default()
                .title(" Find Recipe ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().fg(Color::Rgb(180, 140, 90)));

            let mut spans = Vec::new();
            let pattern_chars: Vec<char> = self.recipe_search.pattern.chars().collect();
            for (i, c) in pattern_chars.iter().enumerate() {
                if i == self.recipe_search.cursor {
                    spans.push(Span::styled(
                        c.to_string(),
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::White)
                            .add_modifier(Modifier::REVERSED),
                    ));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
            if self.recipe_search.cursor == pattern_chars.len() {
                spans.push(Span::styled(
                    " ",
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::REVERSED),
                ));
            }
            f.render_widget(
                Paragraph::new(Text::from(Line::from(spans)))
                    .block(search_block)
                    .alignment(Alignment::Left),
                search_area,
            );

            area.y += search_area.height;
            area.height = area.height.saturating_sub(search_area.height);
        }

        let visible = self.get_visible_recipes();
        let items: Vec<ListItem> = visible
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let is_cursor = self.recipes_menu_state.selected() == Some(i);
                let style = if is_cursor {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                let desc = if r.description.is_empty() {
                    "".to_string()
                } else {
                    format!(" - {}", r.description)
                };
                ListItem::new(format!("▶ {}{}", r.name, desc)).style(style)
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Recipes ({}) ", self.recipes.len())),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_stateful_widget(list, area, &mut self.recipes_menu_state.clone());
    }

    fn render_recipe_form(&self, f: &mut Frame, area: Rect) {
        let labels = ["Name", "Description", "Command"];
        let field_h = area.height / 3;

        let _block_title = match self.recipe_mode {
            RecipeMode::Edit => {
                if self.recipe_edit_target.is_some() {
                    "Edit Recipe"
                } else {
                    "Add New Recipe"
                }
            }
            _ => "Details",
        };

        let title_style = if self.recipe_mode == RecipeMode::Edit {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        for i in 0..3_usize {
            let y_offset = (i as u16) * field_h;
            let block_area = Rect {
                x: area.x,
                y: area.y + y_offset,
                width: area.width,
                height: field_h,
            };

            let block = Block::default()
                .title(Line::from(labels[i]).style(title_style))
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(
                    if i == self.recipe_form.active_field && self.recipe_mode == RecipeMode::Edit {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Rgb(180, 140, 90))
                    },
                );

            let text = self.recipe_form.fields[i].clone();
            let chars: Vec<char> = text.chars().collect();
            let cursor = self.recipe_form.cursors[i];
            let mut spans = vec![];

            let is_focused =
                (i == self.recipe_form.active_field) && (self.recipe_mode == RecipeMode::Edit);

            for (ci, c) in chars.iter().enumerate() {
                let style = if is_focused && ci == cursor {
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::White)
                };
                spans.push(Span::styled(c.to_string(), style));
            }
            if is_focused && cursor == chars.len() {
                spans.push(Span::styled(
                    " ",
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::REVERSED),
                ));
            }
            f.render_widget(
                Paragraph::new(Text::from(Line::from(spans))).block(block),
                block_area,
            );
        }
    }

    fn render_main_menu(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .get_main_menu_items()
            .iter()
            .enumerate()
            .map(|(i, (_id, label))| {
                let is_cursor = self.main_menu_state.selected() == Some(i);
                let style = if is_cursor {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                ListItem::new(label.clone()).style(style)
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Main Menu "))
                .highlight_style(Style::default().bg(Color::DarkGray)),
            area,
            &mut self.main_menu_state.clone(),
        );
    }

    fn render_config_editor(&self, f: &mut Frame, area: Rect) {
        let sort_label = match &self.app_config.sorting {
            SortingMode::FilesFirst => "📄 Files First",
            SortingMode::DirsFirst => "📁 Dirs First",
            _ => "📊 Flat (Alphabetical)",
        };
        let hidden_label = if self.app_config.files.include_hidden {
            "👁️  Show Hidden Files"
        } else {
            "🙈 Hide Hidden Files"
        };
        let options = [sort_label, hidden_label, "💾 Save & Return"];
        let list_items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let is_cursor = self.config_menu_state.selected() == Some(i);
                let style = if is_cursor {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                ListItem::new(format!("  {}", label)).style(style)
            })
            .collect();
        f.render_stateful_widget(
            List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Configuration "),
                )
                .highlight_style(Style::default().bg(Color::DarkGray)),
            area,
            &mut self.config_menu_state.clone(),
        );
    }

    fn render_recipe_action(&self, f: &mut Frame, area: Rect) {
        if self.recipes.is_empty() {
            f.render_widget(
                Paragraph::new("No recipes found\nPress Esc to go back")
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }
        let items: Vec<ListItem> = self
            .recipes
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let style = if self.recipe_action_state.selected() == Some(i) {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                let desc = if r.description.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", r.description)
                };
                ListItem::new(format!("{}{}", r.name, desc)).style(style)
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Select Recipe "),
                )
                .highlight_style(Style::default().bg(Color::DarkGray)),
            area,
            &mut self.recipe_action_state.clone(),
        );
    }

    fn render_output_options(&self, f: &mut Frame, area: Rect) {
        // Updated menu items to match user preference and fix alignment
        let items = [
            ("clipboard", "📋 Copy To Clipboard"),
            ("terminal", "🖨️ Print To Terminal"),
            ("terminal_ln", "🖨️ Print To Terminal (+ Line Numbers)"),
            ("file", "💾 Save To File"),
            ("back", "← Back"),
        ];
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(i, (_id, label))| {
                let style = if self.output_menu_state.selected() == Some(i) {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                ListItem::new(label.to_string()).style(style)
            })
            .collect();
        f.render_stateful_widget(
            List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" What To Do With Output? "),
                )
                .highlight_style(Style::default().bg(Color::DarkGray)),
            area,
            &mut self.output_menu_state.clone(),
        );
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn run() -> Result<()> {
    let mut app = TuiApp::new()?;
    app.run()
}
