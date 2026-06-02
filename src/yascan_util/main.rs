mod html_report;

use std::fs;
use std::io;
use std::path::Path;
use colored::*;
use dialoguer::{Select, theme::ColorfulTheme};
use glob::glob;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const YARA_FORGE_URL: &str = "https://github.com/YARAHQ/yara-forge/releases/latest/download/yara-forge-rules-core.zip";
const SIGNATURES_DIR: &str = "./signatures";
const TEMP_DIR: &str = "./tmp";

// Enable ANSI escape code support on Windows
#[cfg(windows)]
fn enable_ansi_support() {
    use windows::Win32::System::Console::{
        GetStdHandle, SetConsoleMode, GetConsoleMode,
        STD_OUTPUT_HANDLE, STD_ERROR_HANDLE, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    };
    
    unsafe {
        // Enable for stdout
        if let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) {
            let mut mode = std::mem::zeroed();
            if GetConsoleMode(handle, &mut mode).is_ok() {
                let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
        // Enable for stderr
        if let Ok(handle) = GetStdHandle(STD_ERROR_HANDLE) {
            let mut mode = std::mem::zeroed();
            if GetConsoleMode(handle, &mut mode).is_ok() {
                let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi_support() {
    // ANSI codes work natively on Unix-like systems
}

fn main() {
    // Enable ANSI color support on Windows
    enable_ansi_support();
    
    print_banner();
    
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        // Check if running in a TTY (interactive terminal)
        // If not, print usage instead of trying interactive mode
        if atty::is(atty::Stream::Stdin) {
            if let Err(e) = interactive_mode() {
                log_error(&format!("Interactive mode error: {}", e));
                std::process::exit(1);
            }
        } else {
            // Not running in a TTY (e.g., CI/CD, pipes, etc.)
            // Print usage information instead
            print_usage();
        }
        return;
    }

    let command = &args[1];
    match command.as_str() {
        "update" => {
            log_step("Starting signature update...");
            if let Err(e) = update_signatures() {
                log_error(&format!("Error updating signatures: {}", e));
                std::process::exit(1);
            }
            log_success("Signatures updated successfully!");
        }
        "html" => {
            if let Err(e) = handle_html_command(&args[2..]) {
                log_error(&format!("Error generating HTML report: {}", e));
                std::process::exit(1);
            }
        }
        "--help" | "-h" => {
            print_usage();
        }
        _ => {
            log_error(&format!("Unknown command: {}", command));
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_banner() {
    println!("{}", "------------------------------------------------------------------------".bright_green());
    println!("{}", "   ::             x.                                                    ".bright_green());
    println!("{}", "   ;.             xX    ______ _____________ _________                  ".bright_green());
    println!("{}", "   .x            :$x    ___  / __  __ \\__  //_/___  _/                  ".bright_green());
    println!("{}", "    ++           Xx     __  /  _  / / /_  ,<   __  /                    ".bright_green());
    println!("{}", "    .X:  ..;.   ;+.     _  /___/ /_/ /_  /| | __/ /                     ".bright_green());
    println!("{}", "     :xx +XXX;+::.      /_____/\\____/ /_/ |_| /___/                     ".bright_green());
    println!("{}", "       :xx+$;.:.        Yscan  IOC Scanner                          ".bright_green());
    println!("{}", "          .X+:;;                                                        ".bright_green());
    println!("           ;  :.        Version {} (Rust)                               ", VERSION);
    println!("{}", "        .    x+         Yascan 2026                               ".bright_green());
    println!("{}", "         :   +                                                          ".bright_green());
    println!("{}", "------------------------------------------------------------------------".bright_green());
    println!("   Yascan Util {} - YARA & IOC Tools", VERSION);
    println!("{}", "------------------------------------------------------------------------".bright_green());
    println!();
}

fn print_usage() {
    println!("Usage: yascan-util <command>");
    println!();
    println!("Commands:");
    println!("  {}   - 更新 YARA 规则 (YARA-Forge Core)", "update".green());
    println!("  {}    - 从 JSONL 文件生成 HTML 报告", "html".green());
    println!();
    println!("HTML Report Generation:");
    println!("  yascan-util html --input <file.jsonl> --output <report.html>");
    println!("  yascan-util html --input \"*.jsonl\" --combine --output combined.html");
    println!();
    println!("Options:");
    println!("  --input <file|glob>  - 输入 JSONL 文件或 glob 模式");
    println!("  --output <file.html> - 输出 HTML 文件 (可选，默认为 input.html)");
    println!("  --combine            - 合并多个 JSONL 文件为一个报告");
    println!("  --title <str>       - 覆盖报告标题");
    println!("  --host <str>         - 覆盖主机名");
    println!();
}

fn log_info(msg: &str) {
    println!(" {} {}", "[*]".blue(), msg);
}

fn log_success(msg: &str) {
    println!(" {} {}", "[+]".green(), msg);
}

fn log_error(msg: &str) {
    eprintln!(" {} {}", "[!]".red(), msg);
}

fn log_warn(msg: &str) {
    println!(" {} {}", "[!]".yellow(), msg);
}

fn log_step(msg: &str) {
    println!(" {} {}", "[>]".cyan(), msg);
}

fn interactive_mode() -> Result<(), Box<dyn std::error::Error>> {
    let options = vec![
        "Update signatures",
        "Exit"
    ];

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("What would you like to do?")
        .default(0)
        .items(&options)
        .interact()?;

    match selection {
        0 => {
            log_step("Starting signature update...");
            update_signatures()?;
            log_success("Signatures updated successfully!");
        }
        _ => {
            println!("Exiting...");
        }
    }
    
    Ok(())
}

fn update_signatures() -> Result<(), Box<dyn std::error::Error>> {
    // Create signatures directory if it doesn't exist
    fs::create_dir_all(format!("{}/yara", SIGNATURES_DIR))?;
    fs::create_dir_all(format!("{}/iocs", SIGNATURES_DIR))?;
    
    // Create temp directory
    fs::create_dir_all(TEMP_DIR)?;
    
    // Remove existing YARA rules before installing the current Core bundle.
    // This prevents legacy packaged rules from accumulating across updates.
    clear_existing_yara_rules()?;

    // Download and extract YARA rules from yara-forge
    log_info("Downloading YARA rules from yara-forge...");
    download_and_extract_yara_rules()?;

    // Keep IOC files optional, but ensure default placeholder files exist to
    // maintain compatibility with IOC loading paths.
    ensure_default_ioc_files()?;
    
    // Clean up temp directory
    fs::remove_dir_all(TEMP_DIR)?;
    
    Ok(())
}

fn download_file(url: &str, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let resp = ureq::get(url)
        .header("User-Agent", "yascan-util")
        .call()?;
    
    let mut reader = resp.into_body().into_reader();
    let mut file = fs::File::create(output_path)?;
    io::copy(&mut reader, &mut file)?;
    
    Ok(())
}



fn download_and_extract_yara_rules() -> Result<(), Box<dyn std::error::Error>> {
    let zip_path = Path::new(TEMP_DIR).join("yara-forge-rules-core.zip");
    download_file(YARA_FORGE_URL, &zip_path)?;
    
    // Extract ZIP file
    let file = fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    
    let yara_dest = Path::new(SIGNATURES_DIR).join("yara");
    fs::create_dir_all(&yara_dest)?;
    
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        
        // Skip directories
        if file.name().ends_with('/') {
            continue;
        }
        
        // Only extract .yar files
        if !file.name().ends_with(".yar") {
            continue;
        }
        
        // Get the filename from the path
        let file_path = Path::new(file.name());
        let filename = file_path.file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid filename")?;
        
        // Create destination path
        let dest_path = yara_dest.join(filename);
        
        // Extract file directly to signatures/yara
        let mut outfile = fs::File::create(&dest_path)?;
        io::copy(&mut file, &mut outfile)?;
    }
    
    log_success("YARA rules updated from yara-forge");
    
    Ok(())
}

fn clear_existing_yara_rules() -> Result<(), Box<dyn std::error::Error>> {
    let yara_dir = Path::new(SIGNATURES_DIR).join("yara");
    if !yara_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&yara_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("yar") || ext.eq_ignore_ascii_case("yara") {
            fs::remove_file(path)?;
        }
    }

    Ok(())
}

fn ensure_default_ioc_files() -> Result<(), Box<dyn std::error::Error>> {
    let iocs_dir = Path::new(SIGNATURES_DIR).join("iocs");
    fs::create_dir_all(&iocs_dir)?;

    let defaults = [
        ("hash-iocs.txt", "# Optional custom hash IOCs\n"),
        ("filename-iocs.txt", "# Optional custom filename IOCs\n"),
        ("c2-iocs.txt", "# Optional custom C2 IOCs\n"),
        ("keywords.txt", "# Optional custom keyword IOCs\n"),
    ];

    for (name, content) in defaults {
        let path = iocs_dir.join(name);
        if !path.exists() {
            fs::write(path, content)?;
        }
    }

    Ok(())
}

fn handle_html_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut combine = false;
    let mut title: Option<String> = None;
    let mut host: Option<String> = None;
    
    // Parse arguments
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => {
                if i + 1 < args.len() {
                    input = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("--input requires a value".into());
                }
            }
            "--output" | "-o" => {
                if i + 1 < args.len() {
                    output = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("--output requires a value".into());
                }
            }
            "--combine" | "-c" => {
                combine = true;
                i += 1;
            }
            "--title" | "-t" => {
                if i + 1 < args.len() {
                    title = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("--title requires a value".into());
                }
            }
            "--host" | "-h" => {
                if i + 1 < args.len() {
                    host = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("--host requires a value".into());
                }
            }
            "--help" => {
                print_usage();
                return Ok(());
            }
            _ => {
                return Err(format!("Unknown argument: {}", args[i]).into());
            }
        }
    }
    
    let input_path = input.ok_or("--input is required")?;
    
    // Expand glob pattern if needed
    let input_files = expand_inputs(&input_path)?;
    
    if input_files.is_empty() {
        return Err(format!("No files found matching: {}", input_path).into());
    }
    
    log_step(&format!("Found {} JSONL file(s) to process", input_files.len()));
    
    if combine || input_files.len() > 1 {
        // Combined report mode
        log_step("Generating combined HTML report...");
        let combined_data = html_report::parse_multiple_jsonl_files(&input_files)?;
        
        let output_path = output.unwrap_or_else(|| "combined_report.html".to_string());
        let version = combined_data.sources.first()
            .and_then(|s| s.version.as_ref())
            .map(|v| v.clone())
            .unwrap_or_else(|| VERSION.to_string());
        
        html_report::render_combined_html(&combined_data, &version, &output_path)?;
        log_success(&format!("Combined HTML report written to: {}", output_path));
    } else {
        // Single file mode
        log_step("Generating HTML report...");
        let output_path = html_report::generate_single_report(
            &input_files[0],
            output.as_deref(),
            title.as_deref(),
            host.as_deref(),
        )?;
        log_success(&format!("HTML report written to: {}", output_path));
    }
    
    Ok(())
}

fn expand_inputs(pattern: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    
    // Check if pattern contains glob characters
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        // Use glob pattern matching
        let matches = glob(pattern)?;
        for entry in matches {
            match entry {
                Ok(path) => {
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
                Err(e) => {
                    log_warn(&format!("Error matching glob pattern: {}", e));
                }
            }
        }
    } else {
        // Single file path
        let path = Path::new(pattern);
        if !path.exists() {
            return Err(format!("File not found: {}", pattern).into());
        }
        if !path.is_file() {
            return Err(format!("Path is not a file: {}", pattern).into());
        }
        files.push(pattern.to_string());
    }
    
    // Sort for consistent ordering
    files.sort();
    
    Ok(files)
}
