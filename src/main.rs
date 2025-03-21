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
use crate::config::Config;
use dirs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
        return;
    }
    match args[1].as_str() {
        "start" => run_backend(),
        "stop" => stop_backend(),
        _ => usage(),
    }
}

fn usage() {
    println!("Usage: drivedriverb <start|stop>");
}

fn run_backend() {
    println!("DriveDriver starting up...");
    
    // Initialize configuration
    let config_path = get_config_dir().join("config.toml");
    let config = Config::load_or_create(&config_path);
    let config = Arc::new(Mutex::new(config));
    
    // Write PID file so that the stop command can locate this process
    let pid = std::process::id();
    let pid_path = get_config_dir().join("drivedriver.pid");
    let _ = fs::write(&pid_path, pid.to_string());
    
    // Start initial scan in a separate thread
    let scan_config = config.clone();
    let scan_handle = std::thread::spawn(move || {
        scanner::start_initial_scan(scan_config);
    });
    
    // Start API server in the main thread
    let api_config = config.clone();
    api::start_server(api_config);
    
    // Wait for scan to complete
    if let Err(e) = scan_handle.join() {
        eprintln!("Error joining scan thread: {:?}", e);
    }
    
    // Clean up PID file on exit
    let _ = fs::remove_file(pid_path);
}

fn stop_backend() {
    let pid_path = get_config_dir().join("drivedriver.pid");
    if (!pid_path.exists()) {
        println!("No running backend found.");
        return;
    }
    let pid_str = fs::read_to_string(&pid_path)
        .expect("Failed to read PID file");
    println!("Stopping backend with PID: {}", pid_str.trim());
    
    #[cfg(unix)]
    {
        // On Unix, use the kill command.
        let status = Command::new("kill")
            .arg(pid_str.trim())
            .status()
            .expect("Failed to execute kill command");
        if status.success() {
            println!("Backend stopped successfully.");
            let _ = fs::remove_file(pid_path);
        } else {
            println!("Failed to stop backend.");
        }
    }
    
    #[cfg(windows)]
    {
        // On Windows use taskkill.
        let status = Command::new("taskkill")
            .args(&["/PID", pid_str.trim(), "/F"])
            .status()
            .expect("Failed to execute taskkill command");
        if status.success() {
            println!("Backend stopped successfully.");
            let _ = fs::remove_file(pid_path);
        } else {
            println!("Failed to stop backend.");
        }
    }
}

fn get_config_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    let config_dir = home.join(".drivedriver");
    fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    config_dir
}
