use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;
use std::collections::HashMap;
use crate::analyzer::analyze_file;
use crate::storage::FileMetadata;
use crate::config::Config;

pub struct ScanResult {
    pub total_files: usize,
    pub total_size: u64,
    pub file_types: HashMap<String, usize>,
    pub metadata: HashMap<PathBuf, FileMetadata>,
}

pub fn start_initial_scan(config: Arc<Mutex<Config>>) {
    println!("Starting initial scan of all drives...");
    
    // Get all drives
    let drives = get_all_drives();
    
    // Start scanning each drive
    for drive in drives {
        scan_drive(&drive, config.clone());
    }
}

pub fn scan_drive(drive_path: &Path, config: Arc<Mutex<Config>>) -> ScanResult {
    println!("Scanning drive: {:?}", drive_path);
    
    let mut result = ScanResult {
        total_files: 0,
        total_size: 0,
        file_types: HashMap::new(),
        metadata: HashMap::new(),
    };
    
    // Walk the directory tree
    for entry in WalkDir::new(drive_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        
        // Skip excluded paths
        let config_guard = config.lock().unwrap();
        if config_guard.is_path_excluded(path) {
            continue;
        }
        drop(config_guard);
        
        let metadata = match std::fs::metadata(path) {
            Ok(md) => md,
            Err(_) => continue,
        };
        
        // Update scan statistics
        result.total_files += 1;
        result.total_size += metadata.len();
        
        // Get file extension and update file types count
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            *result.file_types.entry(ext_str).or_insert(0) += 1;
        }
        
        // Analyze the file
        let file_metadata = analyze_file(path, &metadata);
        result.metadata.insert(path.to_path_buf(), file_metadata);
    }
    
    // Store results
    let config_dir = crate::get_config_dir();
    let _ = crate::storage::save_scan_result(&config_dir, &result);
    
    result
}

pub fn get_all_drives() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut drives = Vec::new();
        for letter in b'A'..=b'Z' {
            let drive = PathBuf::from(format!("{}:\\", letter as char));
            if drive.exists() {
                drives.push(drive);
            }
        }
        drives
    }
    #[cfg(target_os = "macos")]
    {
        let mut drives = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.filter_map(|e| e.ok()) {
                drives.push(entry.path());
            }
        }
        drives
    }
    #[cfg(target_os = "linux")]
    {
        let mut drives = Vec::new();
        if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let mount_point = parts[1];
                    // Only include real block devices, skip pseudo filesystems
                    if !mount_point.starts_with("/proc") && !mount_point.starts_with("/sys") && !mount_point.starts_with("/dev") && !mount_point.starts_with("/run") {
                        drives.push(PathBuf::from(mount_point));
                    }
                }
            }
        }
        // Add root filesystem if not already present
        if !drives.iter().any(|p| p == Path::new("/")) {
            drives.push(PathBuf::from("/"));
        }
        drives
    }
}
