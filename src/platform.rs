use std::env;
use std::process::Command;

use crate::kitty::KittyTransport;

pub fn running_inside_tmux() -> bool {
    running_inside_tmux_for_env(env::var("TERM").ok(), env::var("TMUX").ok())
}

pub fn running_inside_tmux_for_env(term: Option<String>, tmux: Option<String>) -> bool {
    tmux.is_some() || term.unwrap_or_default().contains("tmux")
}

pub fn likely_supports_kitty_graphics() -> bool {
    likely_supports_kitty_graphics_for_env(
        env::var("TERM").ok(),
        env::var("TERM_PROGRAM").ok(),
        env::var("KITTY_WINDOW_ID").ok(),
        env::var("GHOSTTY_RESOURCES_DIR").ok(),
        env::var("GHOSTTY_BIN_DIR").ok(),
    )
}

pub fn kitty_transport() -> KittyTransport {
    if running_inside_tmux() {
        let (pane_left, pane_top) = tmux_pane_origin().unwrap_or_default();
        KittyTransport::TmuxPassthrough {
            pane_left,
            pane_top,
        }
    } else {
        KittyTransport::Direct
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn kitty_transport_for_env(term: Option<String>, tmux: Option<String>) -> KittyTransport {
    if running_inside_tmux_for_env(term, tmux) {
        KittyTransport::TmuxPassthrough {
            pane_left: 0,
            pane_top: 0,
        }
    } else {
        KittyTransport::Direct
    }
}

fn tmux_pane_origin() -> Option<(u16, u16)> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{pane_left},#{pane_top}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    parse_tmux_pane_origin(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_tmux_pane_origin(output: &str) -> Option<(u16, u16)> {
    let (left, top) = output.trim().split_once(',')?;
    Some((left.parse().ok()?, top.parse().ok()?))
}

pub fn likely_supports_kitty_graphics_for_env(
    term: Option<String>,
    term_program: Option<String>,
    kitty_window_id: Option<String>,
    ghostty_resources_dir: Option<String>,
    ghostty_bin_dir: Option<String>,
) -> bool {
    let term = term.unwrap_or_default();
    let term_program = term_program.unwrap_or_default();

    term.contains("kitty")
        || term.contains("ghostty")
        || kitty_window_id.is_some()
        || matches!(term_program.as_str(), "kitty" | "ghostty")
        || ghostty_resources_dir.is_some()
        || ghostty_bin_dir.is_some()
}
