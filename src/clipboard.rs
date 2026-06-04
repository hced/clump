// src/clipboard.rs
// Cross-platform clipboard integration with Wayland/X11/macOS/Windows support

use anyhow::{Context, Result};
use std::process::Command;

/// Copy text to system clipboard.
///
/// - Linux Wayland: Uses `wl-copy --foreground` if available
/// - Linux X11 / macOS / Windows: Uses `arboard`
///
/// Note: On pure Wayland without wl-clipboard, clipboard may clear after app exit.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // On Wayland, prefer wl-copy for reliable clipboard support
        if is_wayland() && wl_copy_exists() {
            if let Ok(child) = Command::new("wl-copy")
                .arg("--foreground") // Keep content after process exits
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin {
                    use std::io::Write;
                    if stdin.write_all(text.as_bytes()).is_ok() {
                        // Let wl-copy daemonize; don't wait
                        return Ok(());
                    }
                }
            }
        }

        // Fallback: arboard (works on X11, macOS, Windows, and Wayland via XWayland)
        use arboard::Clipboard;
        Clipboard::new()
            .context("Failed to open system clipboard")?
            .set_text(text)
            .context("Failed to write to clipboard")
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        use arboard::Clipboard;
        Clipboard::new()
            .context("Failed to open system clipboard")?
            .set_text(text)
            .context("Failed to write to clipboard")
    }
}

#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn wl_copy_exists() -> bool {
    Command::new("wl-copy")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut c| {
            let _ = c.wait();
            true
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn has_persistent_clipboard_daemon() -> bool {
    // Check if any persistent clipboard daemon is installed
    // This is ONLY for user warnings, NOT for actual copying
    ["clipvault", "cliphist", "cliprust", "stash"]
        .iter()
        .any(|cmd| command_exists(cmd))
}

#[cfg(target_os = "linux")]
fn command_exists(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut c| {
            let _ = c.wait();
            true
        })
        .unwrap_or(false)
}
