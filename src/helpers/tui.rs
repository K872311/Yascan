//! Terminal User Interface (TUI) for Yascan
//! 
//! Provides a fancy CLI interface with:
//! - Left panel: Scan settings
//! - Center pane: Scrolling log output
//! - Status bar: Real-time scan statistics

use std::io::{self, Stdout};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;
use std::collections::VecDeque;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use crate::helpers::interrupt::ScanState;
use crate::helpers::unified_logger::{LogEvent, LogLevel, TuiMessage, EventType};
use crate::helpers::tui_styles::{
    ACCENT_SUCCESS, ACCENT_WARNING, ACCENT_DANGER,
    BG_DARK, FG_PRIMARY, FG_MUTED,
    get_severity_tag_style,
};
use crate::ScanConfig;

const _VERSION: &str = env!("CARGO_PKG_VERSION");

// --- TUI Log Entry (formatted for display) ---

#[derive(Debug, Clone)]
struct LogEntry {
    level: LogLevel,
    message: String,
    timestamp: String,
}

impl LogEntry {
    fn from_event(event: &LogEvent) -> Self {
        let timestamp = event.timestamp.format("%H:%M:%S").to_string();
        
        // Format message based on event type
        let message = match event.event_type {
            EventType::FileMatch => {
                // Format file match with path, score, and first reason
                let path = event.file_path.as_deref().unwrap_or("unknown");
                let score = event.score.unwrap_or(0.0);
                let reason = event.reasons.as_ref()
                    .and_then(|r| r.first())
                    .map(|r| r.message.as_str())
                    .unwrap_or("");
                format!("{} SCORE:{:.0} {}", path, score, reason)
            }
            EventType::ProcessMatch => {
                // Format process match with name, PID, score, and reason
                let name = event.process_name.as_deref().unwrap_or("unknown");
                let pid = event.pid.unwrap_or(0);
                let score = event.score.unwrap_or(0.0);
                let reason = event.reasons.as_ref()
                    .and_then(|r| r.first())
                    .map(|r| r.message.as_str())
                    .unwrap_or("");
                format!("{} PID:{} SCORE:{:.0} {}", name, pid, score, reason)
            }
            _ => {
                // Standard message with context
                if !event.context.is_empty() {
                    let mut msg = event.message.clone();
                    for (k, v) in &event.context {
                        msg.push_str(&format!(" {}={}", k, v));
                    }
                    msg
                } else {
                    event.message.clone()
                }
            }
        };
        
        Self {
            level: event.level,
            message,
            timestamp,
        }
    }
    
    fn level_color(&self) -> Color {
        match self.level {
            LogLevel::Alert => Color::Red,
            LogLevel::Error => Color::Green,
            LogLevel::Warning => Color::Rgb(204, 153, 0),  // Deep yellow
            LogLevel::Notice => Color::Cyan,
            LogLevel::Info => Color::Green,
            LogLevel::Debug => Color::White,
        }
    }
    
    fn level_str(&self) -> &'static str {
        match self.level {
            LogLevel::Alert => "ALERT",
            LogLevel::Error => "ERROR",
            LogLevel::Warning => "WARN",
            LogLevel::Notice => "NOTICE",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
        }
    }
}

// --- Dialog State ---

#[derive(Debug, Clone, PartialEq)]

// --- Scan Settings Display ---

struct SettingsDisplay {
    target_folder: String,
    threads: usize,
    cpu_limit: u8,
    max_file_size: String,
    scan_all_types: bool,
    scan_hard_drives: bool,
    scan_all_drives: bool,
    is_elevated: bool,
    exclusion_count: usize,
    yara_rules_count: usize,
    ioc_count: usize,
}

impl SettingsDisplay {
    fn from_config(config: &ScanConfig, target_folder: &str) -> Self {
        let max_file_size = if config.max_file_size >= 1_000_000 {
            format!("{:.0} MB", config.max_file_size as f64 / 1_000_000.0)
        } else if config.max_file_size >= 1_000 {
            format!("{:.0} KB", config.max_file_size as f64 / 1_000.0)
        } else {
            format!("{} B", config.max_file_size)
        };
        
        Self {
            target_folder: target_folder.to_string(),
            threads: config.threads,
            cpu_limit: config.cpu_limit,
            max_file_size,
            scan_all_types: config.scan_all_types,
            scan_hard_drives: config.scan_hard_drives,
            scan_all_drives: config.scan_all_drives,
            is_elevated: config.is_elevated,
            exclusion_count: config.exclusion_count,
            yara_rules_count: config.yara_rules_count,
            ioc_count: config.ioc_count,
        }
    }
    
    fn truncate_path(&self, max_len: usize) -> String {
        let char_count = self.target_folder.chars().count();
        if char_count <= max_len {
            self.target_folder.clone()
        } else {
            let keep = (max_len.saturating_sub(3)) / 2;
            if keep == 0 {
                return "...".to_string();
            }
            let start: String = self.target_folder.chars().take(keep).collect();
            let end: String = self.target_folder.chars().skip(char_count - keep).collect();
            format!("{}...{}", start, end)
        }
    }
}

// --- TUI Application State ---

pub struct TuiApp {
    logs: VecDeque<LogEntry>,
    max_logs: usize,
    scroll_offset: usize,
    auto_scroll: bool,
    last_visible_height: usize,
    settings: SettingsDisplay,
    scan_state: Arc<ScanState>,
    scan_complete: bool,
    completed_at: Option<std::time::Instant>,
    receiver: Receiver<TuiMessage>,
    // Interactive controls
    show_threads_overlay: bool,
    // Frozen duration when scan completes
    final_duration: Option<Duration>,
    // Loading state during initialization
    is_loading: bool,
    loading_message: String,
    // Spinner animation frame
    spinner_frame: usize,
}

impl TuiApp {
    pub fn new(
        config: &ScanConfig,
        target_folder: &str,
        scan_state: Arc<ScanState>,
        receiver: Receiver<TuiMessage>,
        start_loading: bool,
    ) -> Self {
        Self {
            logs: VecDeque::with_capacity(1000),
            max_logs: 1000,
            scroll_offset: 0,
            auto_scroll: true,
            last_visible_height: 20, // Will be updated on first render
            settings: SettingsDisplay::from_config(config, target_folder),
            scan_state,
            scan_complete: false,
            completed_at: None,
            receiver,
            show_threads_overlay: false,
            final_duration: None,
            is_loading: start_loading,
            loading_message: if start_loading { "Loading IOCs and signatures ...".to_string() } else { String::new() },
            spinner_frame: 0,
        }
    }
    
    fn add_log(&mut self, event: LogEvent) {
        let entry = LogEntry::from_event(&event);
        self.logs.push_back(entry);
        
        // Trim to max size
        while self.logs.len() > self.max_logs {
            self.logs.pop_front();
            // Adjust scroll offset if we removed entries
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
        
        // Auto-scroll to bottom if enabled
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }
    
    fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.auto_scroll = false;
    }
    
    fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.logs.len().saturating_sub(self.last_visible_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);

        // Resume auto-scroll only if at the very bottom
        if max_scroll == 0 || self.scroll_offset >= max_scroll {
            self.auto_scroll = true;
        }
    }
    
    fn scroll_to_bottom(&mut self) {
        // Will be recalculated on render
        self.scroll_offset = usize::MAX;
        self.auto_scroll = true;
    }
    
    fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
    }
    
    fn process_messages(&mut self) {
        // Process all pending messages
        while let Ok(msg) = self.receiver.try_recv() {
            match msg {
                TuiMessage::Log(event) => self.add_log(event),
                TuiMessage::ScanComplete => {
                    self.scan_complete = true;
                    self.completed_at = Some(std::time::Instant::now());
                    // Freeze the timer at completion time
                    self.final_duration = Some(self.scan_state.start_time.elapsed());
                    self.add_log(LogEvent {
                        timestamp: chrono::Utc::now(),
                        level: LogLevel::Info,
                        event_type: EventType::ScanEnd,
                        hostname: String::new(),
                        message: "Scan complete. Exiting...".to_string(),
                        context: std::collections::BTreeMap::new(),
                        file_path: None, pid: None, process_name: None, score: None,
                        file_type: None, file_size: None, md5: None, sha1: None, sha256: None, reasons: None,
                        file_created: None, file_modified: None, file_accessed: None,
                        start_time: None, run_time: None, memory_bytes: None, cpu_usage: None, connection_count: None, listening_ports: None,
                    });
                }
                TuiMessage::InitProgress(message) => {
                    self.loading_message = message;
                }
                TuiMessage::InitComplete { yara_rules_count, ioc_count } => {
                    self.is_loading = false;
                    self.loading_message.clear();
                    // Update settings display with actual counts
                    self.settings.yara_rules_count = yara_rules_count;
                    self.settings.ioc_count = ioc_count;
                }
            }
        }
        
        // Advance spinner animation when loading
        if self.is_loading {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
        }
    }
    
    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        // Ctrl+C or q exits directly
        if modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('c') {
            self.scan_state.should_exit.store(true, std::sync::atomic::Ordering::SeqCst);
            return true;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.scan_state.should_exit.store(true, std::sync::atomic::Ordering::SeqCst);
                return true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
            }
            KeyCode::PageUp => {
                self.scroll_up(10);
            }
            KeyCode::PageDown => {
                self.scroll_down(10);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_to_top();
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_to_bottom();
            }
            KeyCode::Esc => {
                // Close overlay if open, otherwise resume auto-scroll
                if self.show_threads_overlay {
                    self.show_threads_overlay = false;
                } else {
                    self.scroll_to_bottom();
                }
            }
            // --- Interactive Controls ---
            KeyCode::Char('-') | KeyCode::Char('_') => {
                // Decrease CPU limit by 10%
                let new_limit = self.scan_state.adjust_cpu_limit(-10);
                self.settings.cpu_limit = new_limit;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                // Increase CPU limit by 10%
                let new_limit = self.scan_state.adjust_cpu_limit(10);
                self.settings.cpu_limit = new_limit;
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                // Toggle pause
                self.scan_state.toggle_pause();
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // Skip all current elements
                self.scan_state.request_skip();
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                // Toggle thread activity overlay
                self.show_threads_overlay = !self.show_threads_overlay;
            }
            _ => {}
        }

        false
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3);
            }
            _ => {}
        }
    }
}

// --- TUI Rendering ---

fn render_ui(frame: &mut Frame, app: &mut TuiApp) {
    let size = frame.area();

    // Loki-style vertical layout: banner → config → logs → status
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),   // Title + config info
            Constraint::Min(5),     // Log output (full width)
            Constraint::Length(3),  // Status bar
        ])
        .split(size);

    // Banner + config area
    render_banner_area(frame, app, main_chunks[0]);

    // Logs panel (full width)
    render_logs_panel(frame, app, main_chunks[1]);

    // Status bar
    render_status_bar(frame, app, main_chunks[2]);
    
    // Render overlays (in order of priority)
    if app.show_threads_overlay {
        render_threads_overlay(frame, app, size);
    }
}

fn render_banner_area(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let block = Block::default()
        .style(Style::default().bg(BG_DARK));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let max_path_len = (inner.width as usize).saturating_sub(14);
    let target = app.settings.truncate_path(max_path_len);

    // Loki-style: config info in green
    let banner_text = vec![
        Line::from(vec![
            Span::styled("  YaScan ", Style::default().fg(ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
            Span::styled(format!("v{}", _VERSION), Style::default().fg(FG_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("Target: {}  ", target), Style::default().fg(FG_PRIMARY)),
            Span::styled(format!("Threads: {}  ", app.settings.threads), Style::default().fg(FG_PRIMARY)),
            Span::styled(format!("CPU: {}%  ", app.settings.cpu_limit), Style::default().fg(FG_PRIMARY)),
            Span::styled(format!("YARA: {}  ", app.settings.yara_rules_count), Style::default().fg(ACCENT_WARNING).add_modifier(Modifier::BOLD)),
            Span::styled(format!("IOCs: {}  ", app.settings.ioc_count), Style::default().fg(ACCENT_WARNING).add_modifier(Modifier::BOLD)),
            Span::styled(format!("Excl: {}", app.settings.exclusion_count), Style::default().fg(FG_PRIMARY)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("Elevated: {}  ", if app.settings.is_elevated { "YES" } else { "NO" }),
                Style::default().fg(if app.settings.is_elevated { ACCENT_SUCCESS } else { ACCENT_DANGER }).add_modifier(Modifier::BOLD)),
            Span::styled(format!("AllTypes: {}  ", if app.settings.scan_all_types { "ON" } else { "OFF" }),
                Style::default().fg(if app.settings.scan_all_types { ACCENT_SUCCESS } else { FG_MUTED })),
            Span::styled(format!("HardDrv: {}  ", if app.settings.scan_hard_drives { "ON" } else { "OFF" }),
                Style::default().fg(if app.settings.scan_hard_drives { ACCENT_SUCCESS } else { FG_MUTED })),
            Span::styled(format!("AllDrv: {}  ", if app.settings.scan_all_drives { "ON" } else { "OFF" }),
                Style::default().fg(if app.settings.scan_all_drives { ACCENT_SUCCESS } else { FG_MUTED })),
            Span::styled(format!("Skipped: {}  ", app.scan_state.skipped.load(std::sync::atomic::Ordering::Relaxed)),
                Style::default().fg(FG_MUTED)),
            Span::styled(format!("Progress: {:.1}%", app.scan_state.get_progress()),
                Style::default().fg(ACCENT_WARNING).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(" ".repeat(inner.width as usize - 2), Style::default().fg(FG_MUTED).bg(FG_MUTED)),
        ]),
    ];

    let paragraph = Paragraph::new(banner_text)
        .style(Style::default().bg(BG_DARK));

    frame.render_widget(paragraph, inner);
}

fn render_logs_panel(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let inner = area;
    
    let visible_height = inner.height as usize;
    let total_logs = app.logs.len();
    
    // Store visible height for scroll calculations
    app.last_visible_height = visible_height;
    
    // Calculate proper scroll offset
    let max_scroll = total_logs.saturating_sub(visible_height);
    if app.auto_scroll {
        // Auto-scroll: always snap to bottom
        app.scroll_offset = max_scroll;
    } else if app.scroll_offset > max_scroll {
        app.scroll_offset = max_scroll;
    }
    
    // Create list items for visible logs (Loki-style inverted tags)
    let mut items: Vec<ListItem> = Vec::new();

    // Show loading indicator at the top when initializing
    if app.is_loading {
        const SPINNER: &[char] = &['|', '/', '-', '\\'];
        let spinner_char = SPINNER[app.spinner_frame % SPINNER.len()];
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("  {} ", spinner_char), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(&app.loading_message, Style::default().fg(FG_PRIMARY)),
        ])));
    }

    let log_items: Vec<ListItem> = app.logs
        .iter()
        .skip(app.scroll_offset)
        .take(visible_height)
        .map(|entry| {
            // Loki-style inverted tag: colored background + dark text
            let tag_style = get_severity_tag_style(entry.level);
            let level_span = Span::styled(
                format!("{:7}", entry.level_str()),
                tag_style,
            );

            let time_span = Span::styled(
                format!(" {} ", entry.timestamp),
                Style::default().fg(FG_MUTED),
            );

            // Message in severity foreground color
            let msg_color = entry.level_color();
            let max_msg_len = (inner.width as usize).saturating_sub(18);
            let char_count = entry.message.chars().count();
            let msg = if char_count > max_msg_len {
                let truncate_at = max_msg_len.saturating_sub(3);
                format!("{}...", entry.message.chars().take(truncate_at).collect::<String>())
            } else {
                entry.message.clone()
            };

            let msg_span = Span::styled(msg, Style::default().fg(msg_color));

            ListItem::new(Line::from(vec![level_span, time_span, msg_span]))
        })
        .collect();

    items.extend(log_items);

    let list = List::new(items)
        .style(Style::default().bg(BG_DARK));
    
    frame.render_widget(list, inner);
}

fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let files = app.scan_state.files_scanned.load(std::sync::atomic::Ordering::Relaxed);
    let procs = app.scan_state.processes_scanned.load(std::sync::atomic::Ordering::Relaxed);
    let alerts = app.scan_state.alerts.load(std::sync::atomic::Ordering::Relaxed);
    let warnings = app.scan_state.warnings.load(std::sync::atomic::Ordering::Relaxed);
    let notices = app.scan_state.notices.load(std::sync::atomic::Ordering::Relaxed);

    let current_cpu_limit = app.scan_state.get_cpu_limit();
    let is_paused = app.scan_state.is_scan_paused();
    let scan_rate = app.scan_state.update_scan_rate();
    let eta = app.scan_state.get_eta();
    let eta_text = if let Some(eta_secs) = eta {
        let eta_h = eta_secs / 3600;
        let eta_m = (eta_secs % 3600) / 60;
        let eta_s = eta_secs % 60;
        format!("{:02}:{:02}:{:02}", eta_h, eta_m, eta_s)
    } else {
        "--:--:--".to_string()
    };

    let duration = app.final_duration.unwrap_or_else(|| app.scan_state.start_time.elapsed());
    let hours = duration.as_secs() / 3600;
    let mins = (duration.as_secs() % 3600) / 60;
    let secs = duration.as_secs() % 60;

    // Loki-style status indicators
    let pause_indicator = if app.scan_complete {
        Span::styled("[DONE] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD))
    } else if is_paused {
        Span::styled("[PAUSED] ", Style::default().fg(Color::Black).bg(Color::Rgb(204, 153, 0)).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("[RUN] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    };

    let status_text = vec![
        Line::from(vec![
            pause_indicator,
            Span::styled("Files:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", format_number(files)), Style::default().fg(Color::White)),
            Span::styled("Procs:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", format_number(procs)), Style::default().fg(Color::White)),
            Span::styled("| ", Style::default().fg(FG_MUTED)),
            Span::styled("Rate:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:.0}/s ", scan_rate), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("| ", Style::default().fg(FG_MUTED)),
            Span::styled("ETA:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", eta_text), Style::default().fg(Color::White)),
            Span::styled("| ", Style::default().fg(FG_MUTED)),
            Span::styled("ALERT:", Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", alerts), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("WARN:", Style::default().fg(Color::Black).bg(Color::Rgb(204, 153, 0)).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", warnings), Style::default().fg(Color::Rgb(204, 153, 0))),
            Span::styled("NOTICE:", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", notices), Style::default().fg(Color::Cyan)),
            Span::styled("| ", Style::default().fg(FG_MUTED)),
            Span::styled(format!("{:02}:{:02}:{:02}", hours, mins, secs), Style::default().fg(Color::Green)),
            Span::styled(" | ", Style::default().fg(FG_MUTED)),
            Span::styled(format!("CPU:{}% ", current_cpu_limit), Style::default().fg(Color::White)),
            Span::styled("[+/-] ", Style::default().fg(FG_MUTED)),
            Span::styled("[p]ause [s]kip [t]hreads [q]uit", Style::default().fg(FG_MUTED)),
        ]),
    ];

    let paragraph = Paragraph::new(status_text)
        .style(Style::default().bg(BG_DARK))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_threads_overlay(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // Collect current elements from all threads
    let mut entries: Vec<_> = app.scan_state.current_elements.iter()
        .map(|r| (*r.key(), r.value().clone()))
        .collect();
    entries.sort_by_key(|k| k.0);
    
    let active_threads = entries.len();
    let is_paused = app.scan_state.is_scan_paused();
    
    // Calculate overlay size (centered, 80% width, dynamic height based on thread count)
    // Height: entries + empty line + shortcuts + 2 borders = entries + 4
    let overlay_width = (area.width as f32 * 0.8).min(100.0) as u16;
    let overlay_height = (entries.len() as u16 + 4).min(area.height.saturating_sub(4)).max(8);
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);
    
    // Clear the area behind the overlay first (important for proper overlay)
    frame.render_widget(Clear, overlay_area);
    
    // Build title with status (Loki style)
    let title_status = if is_paused {
        Span::styled(" [PAUSED] ", Style::default().fg(Color::Black).bg(Color::Rgb(204, 153, 0)).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [SCANNING] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD))
    };

    let title = Line::from(vec![
        Span::styled(" THREAD ACTIVITY ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        title_status,
        Span::styled(format!(" ({} active) ", active_threads), Style::default().fg(FG_PRIMARY)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FG_MUTED))
        .style(Style::default().bg(BG_DARK));
    
    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);
    
    // Create list items for each thread
    let max_path_len = (inner.width as usize).saturating_sub(12);
    
    let items: Vec<ListItem> = if entries.is_empty() {
        vec![
            ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    if is_paused { "Scan is paused. Press [p] to resume." } else { "(No active scans)" },
                    Style::default().fg(FG_MUTED).add_modifier(Modifier::ITALIC)
                ),
            ])),
        ]
    } else {
        entries.iter().map(|(thread_id, element)| {
            let truncated = truncate_path(element, max_path_len);
            ListItem::new(Line::from(vec![
                Span::styled(format!(" [{:2}] ", thread_id + 1), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(truncated, Style::default().fg(FG_PRIMARY)),
            ]))
        }).collect()
    };

    // Add help text at bottom (Loki style)
    let mut all_items = items;
    all_items.push(ListItem::new(Line::from("")));
    all_items.push(ListItem::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled("[s]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" skip all  ", Style::default().fg(FG_MUTED)),
        Span::styled("[p]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" pause/resume  ", Style::default().fg(FG_MUTED)),
        Span::styled("[Esc/t]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" close", Style::default().fg(FG_MUTED)),
    ])));
    
    let list = List::new(all_items)
        .style(Style::default().bg(BG_DARK));
    
    frame.render_widget(list, inner);
}

/// Truncate path from the middle for display (UTF-8 safe)
fn truncate_path(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len {
        path.to_string()
    } else {
        let keep = (max_len.saturating_sub(3)) / 2;
        if keep == 0 {
            "...".to_string()
        } else {
            let start: String = path.chars().take(keep).collect();
            let end: String = path.chars().skip(char_count - keep).collect();
            format!("{}...{}", start, end)
        }
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len + (len - 1) / 3);
    
    for (i, b) in bytes.iter().enumerate() {
        result.push(*b as char);
        if (len - i - 1) % 3 == 0 && i != len - 1 {
            result.push(',');
        }
    }
    result
}

// --- TUI Runner ---

pub fn run_tui(
    config: &ScanConfig,
    target_folder: &str,
    scan_state: Arc<ScanState>,
    receiver: Receiver<TuiMessage>,
    start_loading: bool,
) -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
    let mut app = TuiApp::new(config, target_folder, scan_state.clone(), receiver, start_loading);
    
    // Main loop - returns (user_quit, scan_was_complete)
    let (user_quit, scan_complete) = run_main_loop(&mut terminal, &mut app);
    
    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
    terminal.show_cursor()?;
    
    // If user quit while scan was still running, force exit the whole process
    // This ensures background scan threads are terminated
    if user_quit && !scan_complete {
        std::process::exit(130); // 130 = 128 + SIGINT (2)
    }
    
    Ok(())
}

/// Returns (user_quit, scan_was_complete)
/// - user_quit: true if user explicitly pressed q+Y, false otherwise
/// - scan_was_complete: true if scan had finished before exit
fn run_main_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut TuiApp,
) -> (bool, bool) {
    loop {
        // Process any pending messages
        app.process_messages();

        // Auto-exit 3 seconds after scan completion
        if let Some(completed_at) = app.completed_at {
            if completed_at.elapsed() >= Duration::from_secs(3) {
                return (false, true);
            }
        }

        // Draw UI
        if terminal.draw(|f| render_ui(f, app)).is_err() {
            return (false, app.scan_complete); // Terminal error
        }
        
        // Poll for events with timeout
        if let Ok(true) = event::poll(Duration::from_millis(50)) {
            match event::read() {
                Ok(Event::Key(key)) => {
                    // Only handle Press events (Windows sends both Press and Release)
                    if key.kind == KeyEventKind::Press {
                        if app.handle_key(key.code, key.modifiers) {
                            return (true, app.scan_complete); // User confirmed quit
                        }
                    }
                }
                Ok(Event::Mouse(mouse)) => {
                    app.handle_mouse(mouse);
                }
                _ => {}
            }
        }
    }
}
