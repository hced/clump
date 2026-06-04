// src/filepicker.rs
// Interactive file selector with fuzzy search, selection toggling, and directory navigation

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::core::is_binary;
use crate::fuzzypicker::FuzzySearch;

pub enum PickerAction {
    Continue,
    Confirmed(Vec<PathBuf>),
    Cancelled,
}

#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

pub struct FilePicker {
    pub root: PathBuf,
    pub current_dir: PathBuf,
    entries: Vec<FileEntry>,
    pub selected: HashSet<PathBuf>,
    state: ListState,
    pub confirmed: bool,
    pub cancelled: bool,
    pub show_hidden: bool,
    pub fuzzy_active: bool,
    pub search: FuzzySearch,
}

impl FilePicker {
    pub fn new(root: &Path) -> Result<Self> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut picker = Self {
            root: root.clone(),
            current_dir: root,
            entries: Vec::new(),
            selected: HashSet::new(),
            state: ListState::default(),
            confirmed: false,
            cancelled: false,
            show_hidden: crate::config::load().files.include_hidden,
            fuzzy_active: false,
            search: FuzzySearch::new(),
        };
        picker.load_entries()?;
        if !picker.entries.is_empty() {
            picker.state.select(Some(0));
        }
        Ok(picker)
    }

    pub fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        for entry in fs::read_dir(&self.current_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            if path.is_file() && is_binary(&path).unwrap_or(true) {
                continue;
            }
            self.entries.push(FileEntry {
                path: path.clone(),
                name,
                is_dir: path.is_dir(),
            });
        }
        self.entries
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        self.refresh_matches();
        if !self.get_visible_entries().is_empty() {
            self.state.select(Some(0));
        }
        Ok(())
    }

    fn refresh_matches(&mut self) {
        let names: Vec<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.search.update_matches(&names);
    }

    fn get_visible_entries(&self) -> Vec<&FileEntry> {
        if self.fuzzy_active {
            self.search
                .matches
                .iter()
                .filter_map(|&i| self.entries.get(i))
                .collect()
        } else {
            self.entries.iter().collect()
        }
    }

    pub fn draw(&self, f: &mut Frame, mut area: Rect) {
        if self.fuzzy_active {
            let search_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 3,
            };
            let search_block = Block::default()
                .title(" Find ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().fg(Color::Rgb(180, 140, 90)));

            let mut spans = Vec::new();
            let pattern_chars: Vec<char> = self.search.pattern.chars().collect();
            for (i, c) in pattern_chars.iter().enumerate() {
                if i == self.search.cursor {
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
            if self.search.cursor == pattern_chars.len() {
                spans.push(Span::styled(
                    " ",
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::REVERSED),
                ));
            }

            let search_widget = Paragraph::new(Text::from(Line::from(spans)))
                .block(search_block)
                .alignment(Alignment::Left);
            f.render_widget(search_widget, search_area);

            area.y += search_area.height;
            area.height = area.height.saturating_sub(search_area.height);
        }

        let visible = self.get_visible_entries();
        let items: Vec<ListItem> = visible
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let is_selected = if e.is_dir {
                    let mut visible_count = 0;
                    let mut selected_count = 0;
                    if let Ok(children) = fs::read_dir(&e.path) {
                        for child in children.flatten() {
                            let c_path = child.path();
                            let c_name = child.file_name().to_string_lossy().to_string();

                            // Respect visibility toggle
                            if !self.show_hidden && c_name.starts_with('.') {
                                continue;
                            }
                            // Skip binaries (they can't be selected, so don't count them)
                            if c_path.is_file() && is_binary(&c_path).unwrap_or(true) {
                                continue;
                            }

                            visible_count += 1;
                            if self.selected.contains(&c_path) {
                                selected_count += 1;
                            }
                        }
                    }
                    if visible_count == 0 {
                        (false, false)
                    } else if selected_count == visible_count {
                        (true, false)
                    } else if selected_count > 0 {
                        (false, true)
                    } else {
                        (false, false)
                    }
                } else {
                    (self.selected.contains(&e.path), false)
                };

                let sel_indicator = if is_selected.0 {
                    "✅ "
                } else if is_selected.1 {
                    "🔹 "
                } else {
                    "   "
                };
                let prefix = if e.is_dir { "📁" } else { "📄" };
                let is_cursor = self.state.selected() == Some(i);
                let style = if is_cursor {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else if is_selected.0 || is_selected.1 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };

                ListItem::new(format!("{}{} {}", sel_indicator, prefix, e.name)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Files "))
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_stateful_widget(list, area, &mut self.state.clone());
    }

    pub fn handle_event(&mut self, key: &event::KeyEvent) -> PickerAction {
        if key.kind != KeyEventKind::Press {
            return PickerAction::Continue;
        }
        let len = self.get_visible_entries().len();

        if self.fuzzy_active {
            match key.code {
                KeyCode::Esc | KeyCode::Tab => {
                    self.fuzzy_active = false;
                    self.state.select(Some(0));
                    return PickerAction::Continue;
                }
                _ => {}
            }

            match key.code {
                KeyCode::Backspace => {
                    self.search.backspace();
                    self.refresh_matches();
                    self.state.select(Some(0));
                    return PickerAction::Continue;
                }
                KeyCode::Delete => {
                    self.search.delete();
                    self.refresh_matches();
                    self.state.select(Some(0));
                    return PickerAction::Continue;
                }
                KeyCode::Up => {
                    if len > 0 {
                        let i = self.state.selected().unwrap_or(0);
                        if i > 0 {
                            self.state.select(Some(i - 1));
                        }
                    }
                }
                KeyCode::Down => {
                    if len > 0 {
                        let i = self.state.selected().unwrap_or(0);
                        if i + 1 < len {
                            self.state.select(Some(i + 1));
                        }
                    }
                }
                KeyCode::Left if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search.move_cursor_left();
                    return PickerAction::Continue;
                }
                KeyCode::Right if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search.move_cursor_right();
                    return PickerAction::Continue;
                }
                KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(parent) = self.current_dir.parent() {
                        self.fuzzy_active = false;
                        self.current_dir = parent.to_path_buf();
                        let _ = self.load_entries();
                    }
                    return PickerAction::Continue;
                }
                KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(i) = self.state.selected() {
                        if let Some(entry) = self.get_visible_entries().get(i) {
                            if entry.is_dir {
                                let next_dir = entry.path.clone();
                                self.fuzzy_active = false;
                                self.current_dir = next_dir;
                                let _ = self.load_entries();
                            }
                        }
                    }
                    return PickerAction::Continue;
                }
                KeyCode::Char('.') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.show_hidden = !self.show_hidden;
                    let _ = self.load_entries();
                    return PickerAction::Continue;
                }
                KeyCode::Char(' ') => {
                    if let Some(i) = self.state.selected() {
                        if let Some(entry) = self.get_visible_entries().get(i) {
                            let path = entry.path.clone();
                            if self.selected.contains(&path) {
                                self.selected.remove(&path);
                                if path.is_dir() {
                                    remove_recursive(&mut self.selected, &path, self.show_hidden);
                                }
                            } else {
                                self.selected.insert(path.clone());
                                if path.is_dir() {
                                    add_recursive(&mut self.selected, &path, self.show_hidden);
                                }
                            }
                        }
                    }
                    return PickerAction::Continue;
                }
                KeyCode::Enter => {
                    self.confirmed = true;
                    // Filter out hidden files if show_hidden is currently false
                    let mut files: Vec<PathBuf> = self
                        .selected
                        .iter()
                        .filter(|p| p.is_file())
                        .cloned()
                        .collect();
                    if !self.show_hidden {
                        files.retain(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| !n.starts_with('.'))
                                .unwrap_or(true)
                        });
                    }
                    return PickerAction::Confirmed(files);
                }
                KeyCode::Char(c) if !c.is_control() => {
                    self.search.handle_input(c);
                    self.refresh_matches();
                    self.state.select(Some(0));
                    return PickerAction::Continue;
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('f') | KeyCode::Tab => {
                    self.fuzzy_active = true;
                    self.search.pattern.clear();
                    self.search.cursor = 0;
                    self.refresh_matches();
                    return PickerAction::Continue;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if len > 0 {
                        let i = self.state.selected().unwrap_or(0);
                        self.state.select(Some((i + 1) % len));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if len > 0 {
                        let i = self.state.selected().unwrap_or(0);
                        self.state
                            .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(i) = self.state.selected() {
                        if let Some(e) = self.entries.get(i) {
                            if e.is_dir {
                                self.current_dir = e.path.clone();
                                let _ = self.load_entries();
                            }
                        }
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(parent) = self.current_dir.parent() {
                        self.current_dir = parent.to_path_buf();
                        let _ = self.load_entries();
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(i) = self.state.selected() {
                        if let Some(e) = self.entries.get(i) {
                            let path = e.path.clone();
                            if self.selected.contains(&path) {
                                self.selected.remove(&path);
                                if path.is_dir() {
                                    remove_recursive(&mut self.selected, &path, self.show_hidden);
                                }
                            } else {
                                self.selected.insert(path.clone());
                                if path.is_dir() {
                                    add_recursive(&mut self.selected, &path, self.show_hidden);
                                }
                            }
                        }
                    }
                }
                KeyCode::Char('.') => {
                    self.show_hidden = !self.show_hidden;
                    let _ = self.load_entries();
                    return PickerAction::Continue;
                }
                KeyCode::Enter => {
                    self.confirmed = true;
                    let mut files: Vec<PathBuf> = self
                        .selected
                        .iter()
                        .filter(|p| p.is_file())
                        .cloned()
                        .collect();
                    if !self.show_hidden {
                        files.retain(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| !n.starts_with('.'))
                                .unwrap_or(true)
                        });
                    }
                    return PickerAction::Confirmed(files);
                }
                KeyCode::Esc => {
                    self.cancelled = true;
                    return PickerAction::Cancelled;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.cancelled = true;
                    return PickerAction::Cancelled;
                }
                _ => {}
            }
        }
        PickerAction::Continue
    }
}

fn add_recursive(selected: &mut HashSet<PathBuf>, dir: &Path, show_hidden: bool) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if path.is_file() && !is_binary(&path).unwrap_or(true) {
                selected.insert(path);
            } else if path.is_dir() {
                selected.insert(path.clone());
                add_recursive(selected, &path, show_hidden);
            }
        }
    }
}

fn remove_recursive(selected: &mut HashSet<PathBuf>, dir: &Path, show_hidden: bool) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            selected.remove(&path);
            if path.is_dir() {
                remove_recursive(selected, &path, show_hidden);
            }
        }
    }
}
