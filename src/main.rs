mod scanner;
mod analyzer;
mod storage;
mod ai_integration;
mod api;
mod config;

use std::sync::{Arc, Mutex};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use crate::config::Config;
use dirs;

fn ensure_installed_in_home() {
    let home = dirs::home_dir().expect("Could not find home directory");
    let target_dir = home.join(".drivedriver");
    let target_path = target_dir.join("drivedriverb");
    // Create target directory if needed
    if let Err(e) = fs::create_dir_all(&target_dir) {
        eprintln!("Failed to create {}: {}", target_dir.display(), e);
    }
    if !target_path.exists() {
        let current_exe = env::current_exe().expect("Failed to get current executable");
        println!("Copying {} to {}", current_exe.display(), target_path.display());
        fs::copy(&current_exe, &target_path)
            .expect("Failed to copy executable to home directory");
        #[cfg(unix)]
        {
            Command::new("chmod")
                .args(&["+x", target_path.to_str().unwrap()])
                .status()
                .expect("Failed to set executable permissions");
        }
    }
}

fn main() {
    // Ensure the executable is installed in home directory
    ensure_installed_in_home();

    let args: Vec<String> = env::args().collect();
    
    // If no arguments, just print version info and exit
    if args.len() < 2 {
        print_version();
        return;
    }

    // Parse port from arguments if provided
    let mut port = 8080; // Default port
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--port" || args[i] == "-p" {
            if i + 1 < args.len() {
                if let Ok(p) = args[i + 1].parse::<u16>() {
                    port = p;
                    println!("Using port: {}", port);
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    
    match args[1].as_str() {
        "start" => run_backend(port, false),
        "stop" => stop_backend(),
        "verbose" => verbose_mode(port),
        "help" => usage(),
        _ => {
            if args[1].starts_with("-") {
                // If it's a flag like -p or --port but not a command
                print_version();
            } else {
                println!("Unknown command: {}", args[1]);
                usage();
            }
        },
    }
}

fn print_version() {
    println!("DriveDriver Backend v{}", env!("CARGO_PKG_VERSION"));
    println!("Run 'drivedriverb help' for usage information.");
}

fn usage() {
    println!("Usage: drivedriverb [COMMAND] [OPTIONS]");
    println!("\nCommands:");
    println!("  start         Start the backend server silently");
    println!("  stop          Stop the running backend server");
    println!("  verbose       Start in verbose mode or connect to running server and display real-time status");
    println!("  help          Display this help message");
    println!("\nOptions:");
    println!("  --port, -p    Specify port number to use (default: 8080)");
    println!("\nExamples:");
    println!("  drivedriverb start --port 8081");
    println!("  drivedriverb verbose");
    println!("  drivedriverb stop");
}

fn verbose_mode(port: u16) {
    // First check if server is already running
    if is_server_running(port) {
        println!("Backend is already running on port {}.", port);
        println!("Connecting to running server for status updates...");
        
        // Start displaying real-time info from existing server
        display_server_status(port);
    } else {
        println!("Backend is not running. Starting in verbose mode...");
        run_backend(port, true);
    }
}

fn is_server_running(port: u16) -> bool {
    // Try to connect to health endpoint
    if let Ok(output) = Command::new("curl")
        .args(&["-s", &format!("http://localhost:{}/health", port)])
        .output() {
        return output.status.success() && !output.stdout.is_empty();
    }
    false
}

fn display_server_status(port: u16) {
    println!("Starting status monitoring. Press Ctrl+C to exit.");
    
    // Continuously poll and display server status
    loop {
        if let Ok(output) = Command::new("curl")
            .args(&["-s", &format!("http://localhost:{}/status", port)])
            .output() {
            if output.status.success() {
                if let Ok(status_str) = String::from_utf8(output.stdout) {
                    // Clear screen and print new status
                    println!("\x1B[2J\x1B[1;1H"); // ANSI clear screen
                    println!("DriveDriver Backend Status (Ctrl+C to exit):");
                    println!("----------------------------------------");
                    println!("{}", status_str);
                }
            } else {
                println!("Lost connection to server. Exiting.");
                break;
            }
        } else {
            println!("Failed to connect to server. Exiting.");
            break;
        }
        
        // Wait before next status update
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn run_backend(port: u16, verbose: bool) {
    if verbose {
        println!("DriveDriver starting up on port {}...", port);
    }
    
    // Initialize configuration
    let config_path = get_config_dir().join("config.json");
    let config = Config::load_or_create(&config_path);
    let config = Arc::new(Mutex::new(config));
    
    // Write PID file so that the stop command can locate this process
    let pid = std::process::id();
    let pid_path = get_config_dir().join("drivedriver.pid");
    let _ = fs::write(&pid_path, format!("{}\n{}", pid, port)); // Store both PID and port
    
    // Start initial scan in a separate thread
    let scan_config = config.clone();
    let scan_handle = std::thread::spawn(move || {
        scanner::start_initial_scan(scan_config);
    });
    
    // Start API server in the main thread
    let api_config = config.clone();
    api::start_server(api_config, port, verbose);
    
    // Wait for scan to complete
    if let Err(e) = scan_handle.join() {
        eprintln!("Error joining scan thread: {:?}", e);
    }
    
    // Clean up PID file on exit
    let _ = fs::remove_file(pid_path);
}

fn stop_backend() {
    let pid_path = get_config_dir().join("drivedriver.pid");
    if !pid_path.exists() {
        println!("No running backend found.");
        return;
    }
    
    let content = match fs::read_to_string(&pid_path) {
        Ok(content) => content,
        Err(_) => {
            println!("Failed to read PID file.");
            return;
        }
    };
    
    let parts: Vec<&str> = content.trim().split('\n').collect();
    let pid_str = parts[0];
    
    #[cfg(unix)]
    {
        // On Unix, use the kill command.
        let status = Command::new("kill")
            .arg(pid_str)
            .status()
            .expect("Failed to execute kill command");
        if status.success() {
            let _ = fs::remove_file(pid_path);
        }
    }
    
    #[cfg(windows)]
    {
        // On Windows use taskkill.
        let status = Command::new("taskkill")
            .args(&["/PID", pid_str, "/F"])
            .status()
            .expect("Failed to execute taskkill command");
        if status.success() {
            let _ = fs::remove_file(pid_path);
        }
    }
}

fn get_config_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    let config_dir = home.join(".drivedriverb");
    fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    config_dir
}
