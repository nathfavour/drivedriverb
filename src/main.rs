mod scanner;
mod analyzer;
mod storage;
mod ai_integration;
mod api;
mod config;

use std::sync::{Arc, Mutex};
use crate::config::Config;
use dirs;

fn main() {
    println!("DriveDriver starting up...");
    
    // Initialize configuration
    let config_path = get_config_dir().join("config.toml");
    let config = Config::load_or_create(&config_path);
    let config = Arc::new(Mutex::new(config));
    
    // Start the API server for Flutter frontend
    let api_handle = std::thread::spawn(move || {
        api::start_server(config.clone());
    });
    
    // Start initial scan
    scanner::start_initial_scan(config.clone());
    
    // Block on API server
    api_handle.join().unwrap();
}

fn get_config_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    let config_dir = home.join(".drivedriver");
    std::fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    config_dir
}
