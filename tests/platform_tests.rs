use termpdf::kitty::KittyTransport;
use termpdf::platform::{
    kitty_transport_for_env, likely_supports_kitty_graphics_for_env, running_inside_tmux_for_env,
};

#[test]
fn detects_tmux_by_term_name() {
    assert!(running_inside_tmux_for_env(
        Some("tmux-256color".to_string()),
        None,
    ));
}

#[test]
fn detects_tmux_by_tmux_env() {
    assert!(running_inside_tmux_for_env(
        Some("xterm-256color".to_string()),
        Some("/tmp/tmux-1000/default,123,0".to_string()),
    ));
}

#[test]
fn rejects_non_tmux_terminal() {
    assert!(!running_inside_tmux_for_env(
        Some("xterm-256color".to_string()),
        None,
    ));
}

#[test]
fn selects_tmux_passthrough_transport_inside_tmux() {
    assert_eq!(
        kitty_transport_for_env(
            Some("tmux-256color".to_string()),
            Some("/tmp/tmux-1000/default,123,0".to_string()),
        ),
        KittyTransport::TmuxPassthrough {
            pane_left: 0,
            pane_top: 0,
        },
    );
}

#[test]
fn selects_direct_transport_outside_tmux() {
    assert_eq!(
        kitty_transport_for_env(Some("xterm-kitty".to_string()), None),
        KittyTransport::Direct,
    );
}

#[test]
fn treats_ghostty_env_inside_tmux_as_likely_kitty_graphics_support() {
    assert!(likely_supports_kitty_graphics_for_env(
        Some("tmux-256color".to_string()),
        Some("tmux".to_string()),
        None,
        Some("/Applications/Ghostty.app/Contents/Resources/ghostty".to_string()),
        Some("/Applications/Ghostty.app/Contents/MacOS".to_string()),
    ));
}

#[test]
fn rejects_plain_terminal_as_not_likely_supported() {
    assert!(!likely_supports_kitty_graphics_for_env(
        Some("xterm-256color".to_string()),
        Some("Apple_Terminal".to_string()),
        None,
        None,
        None,
    ));
}
