// src/fuzzypicker.rs
// Fuzzy matching engine using nucleo for real-time file filtering

use nucleo::{Config, Matcher, Utf32Str};
use std::panic;

pub struct FuzzySearch {
    pub pattern: String,
    pub cursor: usize,
    pub matches: Vec<usize>, // Indices into the parent file list
    matcher: Matcher,
    needle_buffer: Vec<char>,
}

impl FuzzySearch {
    pub fn new() -> Self {
        Self {
            pattern: String::new(),
            cursor: 0,
            matches: Vec::new(),
            matcher: Matcher::new(Config::DEFAULT),
            needle_buffer: Vec::with_capacity(64),
        }
    }

    pub fn update_matches(&mut self, file_names: &[&str]) {
        self.matches.clear();
        if self.pattern.is_empty() {
            self.matches = (0..file_names.len()).collect();
            return;
        }

        let sanitized_pattern = self.pattern.trim().to_lowercase();
        if sanitized_pattern.is_empty() {
            self.matches = (0..file_names.len()).collect();
            return;
        }

        for (idx, name) in file_names.iter().enumerate() {
            let matched = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                let needle = Utf32Str::new(&sanitized_pattern, &mut self.needle_buffer);
                let mut haystack_buf = Vec::with_capacity(name.len());
                let haystack = Utf32Str::new(name, &mut haystack_buf);
                self.matcher.fuzzy_match(haystack, needle).is_some()
            }))
            .unwrap_or_else(|_| {
                // Fallback: simple case-insensitive substring match
                name.to_lowercase().contains(&sanitized_pattern)
            });

            if matched {
                self.matches.push(idx);
            }
        }
    }

    pub fn handle_input(&mut self, c: char) {
        if self.cursor == self.pattern.len() {
            self.pattern.push(c);
            self.cursor += 1;
        } else {
            self.pattern.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.pattern.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.pattern.len() {
            self.pattern.remove(self.cursor);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.pattern.len() {
            self.cursor += 1;
        }
    }
}
