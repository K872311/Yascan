//! TUI Style Definitions
//! Loki-inspired semantic color scheme for Yascan

#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

// Loki-style semantic colors
pub const COLOR_ALERT: Color = Color::Red;
pub const COLOR_WARNING: Color = Color::Rgb(204, 153, 0);   // Deep yellow / dark gold
pub const COLOR_NOTICE: Color = Color::Cyan;
pub const COLOR_INFO: Color = Color::Green;
pub const COLOR_ERROR: Color = Color::Green;
pub const COLOR_DEBUG: Color = Color::White;

// Background highlight colors (for inverted tags)
pub const BG_ALERT: Color = Color::Red;
pub const BG_WARNING: Color = Color::Rgb(204, 153, 0);      // Deep yellow / dark gold
pub const BG_NOTICE: Color = Color::Cyan;
pub const BG_INFO: Color = Color::Green;
pub const BG_ERROR: Color = Color::Green;
pub const BG_DEBUG: Color = Color::White;

// Neutral colors
pub const BG_DARK: Color = Color::Black;
pub const BG_PANEL: Color = Color::Black;
pub const FG_PRIMARY: Color = Color::White;
pub const FG_SECONDARY: Color = Color::Green;
pub const FG_MUTED: Color = Color::DarkGray;
pub const BORDER_COLOR: Color = Color::DarkGray;

// Aliases for compatibility
pub const ACCENT_PRIMARY: Color = Color::Green;
pub const ACCENT_SECONDARY: Color = Color::Cyan;
pub const ACCENT_SUCCESS: Color = Color::Green;
pub const ACCENT_WARNING: Color = Color::Rgb(204, 153, 0);  // Deep yellow / dark gold
pub const ACCENT_DANGER: Color = Color::Red;
pub const ACCENT_INFO: Color = Color::Cyan;

// Severity (same as semantic)
pub const SEVERITY_CRITICAL: Color = Color::Red;
pub const SEVERITY_HIGH: Color = Color::Yellow;
pub const SEVERITY_MEDIUM: Color = Color::Yellow;
pub const SEVERITY_LOW: Color = Color::Cyan;

/// Header style for titles (Loki-style bright white bold)
pub fn header_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// Panel border style
pub fn panel_border_style() -> Style {
    Style::default().fg(FG_MUTED)
}

/// Active/focused border style
pub fn active_border_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// Highlight style for important info
pub fn highlight_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// Dim/muted text
pub fn muted_style() -> Style {
    Style::default().fg(FG_MUTED)
}

/// Style for labels (Loki-style green)
pub fn label_style() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

/// Style for values
pub fn value_style() -> Style {
    Style::default().fg(FG_PRIMARY)
}

/// Status indicator styles
pub fn status_running_style() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

pub fn status_paused_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(204, 153, 0))
        .add_modifier(Modifier::BOLD)
}

pub fn status_complete_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

/// Loki-style severity: foreground color for log text
pub fn get_severity_style(level: crate::helpers::unified_logger::LogLevel) -> Style {
    use crate::helpers::unified_logger::LogLevel;
    match level {
        LogLevel::Alert => Style::default().fg(COLOR_ALERT).add_modifier(Modifier::BOLD),
        LogLevel::Error => Style::default().fg(COLOR_ERROR).add_modifier(Modifier::BOLD),
        LogLevel::Warning => Style::default().fg(COLOR_WARNING).add_modifier(Modifier::BOLD),
        LogLevel::Notice => Style::default().fg(COLOR_NOTICE),
        LogLevel::Info => Style::default().fg(COLOR_INFO),
        LogLevel::Debug => Style::default().fg(COLOR_DEBUG),
    }
}

/// Loki-style inverted tag: colored background + dark text (for [ALERT] etc.)
pub fn get_severity_tag_style(level: crate::helpers::unified_logger::LogLevel) -> Style {
    use crate::helpers::unified_logger::LogLevel;
    match level {
        LogLevel::Alert => Style::default().fg(Color::White).bg(BG_ALERT).add_modifier(Modifier::BOLD),
        LogLevel::Error => Style::default().fg(Color::Black).bg(BG_ERROR).add_modifier(Modifier::BOLD),
        LogLevel::Warning => Style::default().fg(Color::Black).bg(BG_WARNING).add_modifier(Modifier::BOLD),
        LogLevel::Notice => Style::default().fg(Color::Black).bg(BG_NOTICE).add_modifier(Modifier::BOLD),
        LogLevel::Info => Style::default().fg(Color::Black).bg(BG_INFO).add_modifier(Modifier::BOLD),
        LogLevel::Debug => Style::default().fg(Color::Black).bg(BG_DEBUG),
    }
}

/// Get severity color
pub fn get_severity_color(level: crate::helpers::unified_logger::LogLevel) -> Color {
    use crate::helpers::unified_logger::LogLevel;
    match level {
        LogLevel::Alert => COLOR_ALERT,
        LogLevel::Error => COLOR_ERROR,
        LogLevel::Warning => COLOR_WARNING,
        LogLevel::Notice => COLOR_NOTICE,
        LogLevel::Info => COLOR_INFO,
        LogLevel::Debug => COLOR_DEBUG,
    }
}

/// Badge style for counts
pub fn badge_style(color: Color) -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

/// Pulsing border (subtle in Loki style)
pub fn pulsing_border_style(_frame: usize) -> Style {
    Style::default().fg(FG_MUTED)
}
