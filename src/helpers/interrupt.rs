use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use dashmap::DashMap;
use std::io::{self, Write};
use colored::Colorize;
use rayon::current_thread_index;

pub struct ScanState {
    pub current_elements: DashMap<usize, String>,
    pub files_scanned: AtomicUsize,
    pub files_total: AtomicUsize,
    pub processes_scanned: AtomicUsize,
    pub skipped: AtomicUsize,
    pub alerts: AtomicUsize,
    pub warnings: AtomicUsize,
    pub notices: AtomicUsize,
    pub errors: AtomicUsize,
    pub start_time: Instant,
    pub should_exit: AtomicBool,
    pub menu_active: AtomicBool,
    // TUI interactive controls
    pub cpu_limit: AtomicU8,
    pub is_paused: AtomicBool,
    pub skip_generation: AtomicU64,
    // Performance metrics
    last_update_time: std::sync::Mutex<Instant>,
    last_scanned_count: std::sync::Mutex<usize>,
    scan_rate: std::sync::Mutex<f64>, // files per second
}

impl ScanState {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_cpu_limit(100)
    }

    pub fn with_cpu_limit(cpu_limit: u8) -> Self {
        let now = Instant::now();
        Self {
            current_elements: DashMap::new(),
            files_scanned: AtomicUsize::new(0),
            files_total: AtomicUsize::new(0),
            processes_scanned: AtomicUsize::new(0),
            skipped: AtomicUsize::new(0),
            alerts: AtomicUsize::new(0),
            warnings: AtomicUsize::new(0),
            notices: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            start_time: now,
            should_exit: AtomicBool::new(false),
            menu_active: AtomicBool::new(false),
            cpu_limit: AtomicU8::new(cpu_limit.clamp(1, 100)),
            is_paused: AtomicBool::new(false),
            skip_generation: AtomicU64::new(0),
            last_update_time: std::sync::Mutex::new(now),
            last_scanned_count: std::sync::Mutex::new(0),
            scan_rate: std::sync::Mutex::new(0.0),
        }
    }

    pub fn set_current_element(&self, element: String) {
        if let Some(idx) = current_thread_index() {
            self.current_elements.insert(idx, element);
        }
    }

    pub fn clear_current_element(&self) {
        if let Some(idx) = current_thread_index() {
            self.current_elements.remove(&idx);
        }
    }

    pub fn increment_files(&self) {
        self.files_scanned.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_processes(&self) {
        self.processes_scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_alerts(&self, count: usize) {
        self.alerts.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_warnings(&self, count: usize) {
        self.warnings.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_notices(&self, count: usize) {
        self.notices.fetch_add(count, Ordering::Relaxed);
    }

    pub fn increment_errors(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_files_total(&self, total: usize) {
        self.files_total.store(total, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_files_total(&self) -> usize {
        self.files_total.load(Ordering::Relaxed)
    }

    pub fn get_progress(&self) -> f32 {
        let scanned = self.files_scanned.load(Ordering::Relaxed);
        let total = self.files_total.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            let progress = (scanned as f32 / total as f32) * 100.0;
            progress.min(100.0)  // Cap at 100% to avoid显示超过100%的情况
        }
    }

    /// Calculate scan rate (files per second) and update internal state
    pub fn update_scan_rate(&self) -> f64 {
        let scanned = self.files_scanned.load(Ordering::Relaxed);
        let now = Instant::now();

        let mut last_count = self.last_scanned_count.lock().unwrap();
        let mut last_time = self.last_update_time.lock().unwrap();
        let mut scan_rate = self.scan_rate.lock().unwrap();

        let elapsed = now.duration_since(*last_time).as_secs_f64();
        if elapsed >= 1.0 {
            let delta = scanned.saturating_sub(*last_count) as f64;
            *scan_rate = delta / elapsed;
            *last_count = scanned;
            *last_time = now;
        }

        *scan_rate
    }

    /// Get current scan rate (files/sec)
    pub fn get_scan_rate(&self) -> f64 {
        *self.scan_rate.lock().unwrap()
    }

    /// Calculate estimated time of arrival (ETA) in seconds
    pub fn get_eta(&self) -> Option<u64> {
        let scanned = self.files_scanned.load(Ordering::Relaxed);
        let total = self.files_total.load(Ordering::Relaxed);
        let rate = self.get_scan_rate();

        if rate > 0.0 && scanned < total {
            let remaining = total.saturating_sub(scanned) as f64;
            Some((remaining / rate) as u64)
        } else {
            None
        }
    }

    pub fn should_stop(&self) -> bool {
        self.should_exit.load(Ordering::Relaxed)
    }

    pub fn wait_for_resume(&self) {
        while (self.menu_active.load(Ordering::Relaxed) || self.is_paused.load(Ordering::Relaxed)) 
              && !self.should_exit.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // --- TUI Interactive Control Methods ---

    /// Get current CPU limit percentage (1-100)
    pub fn get_cpu_limit(&self) -> u8 {
        self.cpu_limit.load(Ordering::Relaxed)
    }

    /// Set CPU limit percentage (clamped to 1-100)
    #[allow(dead_code)]
    pub fn set_cpu_limit(&self, limit: u8) {
        self.cpu_limit.store(limit.clamp(1, 100), Ordering::Relaxed);
    }

    /// Adjust CPU limit by delta (can be negative), returns new value
    pub fn adjust_cpu_limit(&self, delta: i8) -> u8 {
        let current = self.cpu_limit.load(Ordering::Relaxed) as i16;
        let new_limit = (current + delta as i16).clamp(1, 100) as u8;
        self.cpu_limit.store(new_limit, Ordering::Relaxed);
        new_limit
    }

    /// Check if scan is paused
    pub fn is_scan_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    /// Toggle pause state, returns new state (true = paused)
    pub fn toggle_pause(&self) -> bool {
        // Flip the boolean atomically
        let was_paused = self.is_paused.fetch_xor(true, Ordering::SeqCst);
        !was_paused
    }

    /// Request all threads to skip their current element
    /// Increments skip_generation which signals threads to abandon current work
    pub fn request_skip(&self) {
        self.skip_generation.fetch_add(1, Ordering::SeqCst);
        // Also clear all current elements since they're being skipped
        self.current_elements.clear();
    }

    /// Get current skip generation counter
    #[allow(dead_code)]
    pub fn get_skip_generation(&self) -> u64 {
        self.skip_generation.load(Ordering::Relaxed)
    }

    /// Check if thread should skip (generation changed) and update thread's generation
    /// Returns true if the thread should skip its current work
    #[allow(dead_code)]
    pub fn should_skip(&self, thread_generation: &mut u64) -> bool {
        let current_gen = self.skip_generation.load(Ordering::Relaxed);
        if current_gen != *thread_generation {
            *thread_generation = current_gen;
            true
        } else {
            false
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
                result.push('.');
            }
        }
        result
    }

    fn truncate_middle(s: &str, max_len: usize) -> String {
        let char_count = s.chars().count();
        if char_count <= max_len {
            return s.to_string();
        }
        
        let keep_len = (max_len.saturating_sub(3)) / 2;
        if keep_len == 0 {
            return "...".to_string();
        }
        let start: String = s.chars().take(keep_len).collect();
        let end: String = s.chars().skip(char_count - keep_len).collect();
        
        format!("{}...{}", start, end)
    }

    pub fn display_menu(&self) {
        // Set menu active to pause output from other threads if possible
        self.menu_active.store(true, Ordering::SeqCst);
        
        // Clear line to prevent mess
        print!("\r");
        
        let duration = self.start_time.elapsed();
        let files = self.files_scanned.load(Ordering::Relaxed);
        let procs = self.processes_scanned.load(Ordering::Relaxed);
        let alerts = self.alerts.load(Ordering::Relaxed);
        let warnings = self.warnings.load(Ordering::Relaxed);
        let notices = self.notices.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        
        println!("{}", "                                                        ");
        println!("{}", "                                                        ".on_green());
        println!("{}", "                  YASCAN INTERRUPT MENU                   ".black().on_green().bold());
        println!("{}", "                                                        ".on_green());
        
        println!("\nSCAN STATISTICS:");
        println!("  Duration:          {:02}:{:02}:{:02}", 
            duration.as_secs() / 3600,
            (duration.as_secs() % 3600) / 60,
            duration.as_secs() % 60);
        println!("  Files scanned:     {}", Self::format_number(files));
        println!("  Processes scanned: {}", Self::format_number(procs));
        println!("  Alerts:            {}", Self::format_number(alerts).red().bold());
        println!("  Warnings:          {}", Self::format_number(warnings).yellow().bold());
        println!("  Notices:           {}", Self::format_number(notices).cyan());
        println!("  Errors:            {}", Self::format_number(errors).purple());
        
        println!("\nCURRENTLY SCANNING ({} threads active):", self.current_elements.len());
        
        // Collect and sort entries by thread ID for consistent display
        let mut entries: Vec<_> = self.current_elements.iter()
            .map(|r| (*r.key(), r.value().clone()))
            .collect();
        entries.sort_by_key(|k| k.0);
        
        if entries.is_empty() {
             println!("  (No active scans)");
        } else {
            for (thread_id, element) in entries {
                let display_element = Self::truncate_middle(&element, 75);
                println!("  [{}] {}", thread_id, display_element);
            }
        }
        
        println!("\n{}", "-".repeat(60).bright_black());
        println!("  [E] Exit gracefully    [X] Exit immediately    [C] Continue scan");
        println!("{}", "-".repeat(60).bright_black());
        
        print!("> ");
        io::stdout().flush().unwrap();
        
        // Simple input loop
        loop {
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let cmd = input.trim().to_uppercase();
                    match cmd.as_str() {
                        "E" | "EXIT" => {
                            println!("Exiting gracefully... (please wait)");
                            self.should_exit.store(true, Ordering::SeqCst);
                            break;
                        },
                        "X" | "KILL" | "QUIT" => {
                            println!("Exiting immediately...");
                            std::process::exit(0);
                        },
                        "C" | "CONTINUE" => {
                            println!("Resuming scan...");
                            break;
                        },
                        _ => {
                            print!("Invalid option. [E]xit, [X] immediate or [C]ontinue > ");
                            io::stdout().flush().unwrap();
                        }
                    }
                },
                Err(_) => {
                    // If reading stdin fails, default to exit
                    self.should_exit.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        
        self.menu_active.store(false, Ordering::SeqCst);
    }
}
