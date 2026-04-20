use std::env;

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
