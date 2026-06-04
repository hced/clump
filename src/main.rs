// src/main.rs
// Entry point: initializes and runs the CLI/TUI dispatcher

mod cli;
mod clipboard;
mod config;
mod core;
mod filepicker;
mod fuzzypicker;
mod recipe;

#[cfg(feature = "tui")]
mod tui;

use anyhow::Result;

fn main() -> Result<()> {
    cli::dispatch()
}
