use std::sync::{Arc, Mutex};
use actix_web::{web, App, HttpResponse, HttpServer, Responder, middleware::Logger};
use actix_cors::Cors;
use serde_derive::{Serialize, Deserialize};
use crate::config::Config;
use crate::scanner;
use crate::storage;
use std::thread;
use std::path::PathBuf;
use tokio;

// API response types
#[derive(Serialize)]
struct StatusResponse {
    status: String,
    version: String,
}

#[derive(Serialize)]
struct DriveInfoResponse {
    drives: Vec<String>,
}

#[derive(Serialize)]
struct ScanResultResponse {
    total_files: usize,
    total_size: u64,
    file_types: Vec<FileTypeInfo>,
}

#[derive(Serialize)]
struct FileTypeInfo {
    extension: String,
    count: usize,
}

#[derive(Deserialize)]
struct ScanDriveRequest {
    path: String,
}

#[derive(Serialize)]
struct DriveDetail {
    mount_point: String,
    fs_type: String,
    total_space: u64,
    available_space: u64,
    used_space: u64,
    is_removable: bool,
}

#[derive(Deserialize)]
struct FileOpRequest {
    path: String,
    new_path: Option<String>,
    content: Option<String>,
}

async fn health_check() -> impl Responder {
    let response = StatusResponse {
        status: "running".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    HttpResponse::Ok().json(response)
}

async fn get_drives() -> impl Responder {
    let drives = scanner::get_all_drives();
    let mut details = Vec::new();
    for drive in drives {
        let stat = nix::sys::statvfs::statvfs(&drive).ok();
        let (total, avail, used) = if let Some(stat) = stat {
            let total = stat.blocks() * stat.block_size();
            let avail = stat.blocks_available() * stat.block_size();
            let used = total - avail;
            (total, avail, used)
        } else { (0, 0, 0) };
        let fs_type = "unknown".to_string(); // Optionally detect fs type
        let is_removable = drive.to_string_lossy().starts_with("/media/") || drive.to_string_lossy().starts_with("/mnt/");
        details.push(DriveDetail {
            mount_point: drive.to_string_lossy().to_string(),
            fs_type,
            total_space: total,
            available_space: avail,
            used_space: used,
            is_removable,
        });
    }
    HttpResponse::Ok().json(details)
}

async fn get_scan_stats() -> impl Responder {
    // Get the stats from the latest scan
    let config_dir = crate::get_config_dir();
    let stats_path = config_dir.join("data").join("latest_stats.json");
    
    if let Ok(content) = std::fs::read_to_string(stats_path) {
        if let Ok(stats) = serde_json::from_str::<serde_json::Value>(&content) {
            return HttpResponse::Ok().json(stats);
        }
    }
    
    // Return empty stats if no data available
    HttpResponse::Ok().json(serde_json::json!({
        "timestamp": 0,
        "total_files": 0,
        "total_size": 0,
        "file_types": {}
    }))
}

async fn initiate_scan(data: web::Json<ScanDriveRequest>, config: web::Data<Arc<Mutex<Config>>>) -> impl Responder {
    let path = PathBuf::from(&data.path);
    
    // Start a scan in a background thread
    let config_clone = config.get_ref().clone();
    thread::spawn(move || {
        println!("API triggered scan of drive: {:?}", path);
        let _ = scanner::scan_drive(&path, config_clone);
    });
    
    HttpResponse::Ok().json(serde_json::json!({
        "status": "started",
        "path": data.path
    }))
}

async fn get_metadata(_config: web::Data<Arc<Mutex<Config>>>) -> impl Responder {
    // Load file metadata
    let config_dir = crate::get_config_dir();
    match storage::load_file_metadata(&config_dir) {
        Ok(metadata) => {
            // Convert metadata to a list format
            let simplified: Vec<serde_json::Value> = metadata.values()
                .map(|meta| {
                    serde_json::json!({
                        "path": meta.path.to_string_lossy(),
                        "name": meta.file_name,
                        "size": meta.size,
                        "category": meta.category,
                        "importance": meta.importance_score,
                    })
                })
                .collect();
            
            HttpResponse::Ok().json(simplified) // Return as a list
        },
        Err(_) => {
            // Return an empty list instead of a map to match frontend expectations
            HttpResponse::Ok().json(Vec::<serde_json::Value>::new())
        }
    }
}

// Add a new endpoint for system status that includes more details
async fn get_system_status() -> impl Responder {
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
        
    let memory_usage = get_memory_usage();
    let cpu_usage = get_cpu_usage();
    
    // Add more detailed statistics
    let scan_stats = get_scan_statistics();
    
    let response = serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime": uptime,
        "memory_usage": memory_usage,
        "cpu_usage": cpu_usage,
        "scan_stats": scan_stats,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "process_id": std::process::id(),
    });
    
    HttpResponse::Ok().json(response)
}

// Add config management endpoints
async fn get_config() -> impl Responder {
    let config_dir = crate::get_config_dir();
    let config_path = config_dir.join("config.json");
    
    if let Ok(content) = std::fs::read_to_string(config_path) {
        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
            return HttpResponse::Ok().json(config);
        }
    }
    
    // Return default config if file doesn't exist
    HttpResponse::Ok().json(get_default_config())
}

async fn update_config(data: web::Json<serde_json::Value>) -> impl Responder {
    let config_dir = crate::get_config_dir();
    let config_path = config_dir.join("config.json");
    
    // Validate config before saving
    if !validate_config(&data) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Invalid configuration"
        }));
    }
    
    // Save config to file
    match std::fs::write(&config_path, serde_json::to_string_pretty(&data).unwrap()) {
        Ok(_) => {
            // Notify config change to running processes
            notify_config_change();
            HttpResponse::Ok().json(serde_json::json!({
                "status": "success",
                "message": "Configuration updated successfully"
            }))
        },
        Err(e) => {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to save configuration: {}", e)
            }))
        }
    }
}

// Helper function to validate configuration
fn validate_config(config: &serde_json::Value) -> bool {
    // Perform validation checks here
    if let Some(obj) = config.as_object() {
        // Check required fields
        if !obj.contains_key("scan_mode") || !obj.contains_key("excluded_paths") {
            return false;
        }
        
        // Validate scan_mode
        if let Some(scan_mode) = obj.get("scan_mode").and_then(|m| m.as_str()) {
            if scan_mode != "sequential" && scan_mode != "concurrent" {
                return false;
            }
        } else {
            return false;
        }
        
        // Add more validation as needed
        return true;
    }
    false
}

// Get default configuration
fn get_default_config() -> serde_json::Value {
    serde_json::json!({
        "scan_mode": "sequential",
        "excluded_paths": [],
        "max_concurrent_scans": 4,
        "analyze_content": true,
        "use_ai_analysis": false,
        "scan_interval_hours": 24,
        "notification_enabled": true,
        "ui_theme": "system",
        "file_preview_enabled": true
    })
}

// Notify system of config change
fn notify_config_change() {
    // Implementation would depend on how you want to handle live config changes
    println!("Configuration changed, notifying system...");
    // You could use a channel, a shared state, or other mechanisms here
}

// Helper functions for system monitoring
fn get_memory_usage() -> u64 {
    // This is a simplified implementation
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
            for line in content.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(mem_str) = line.split_whitespace().nth(1) {
                        if let Ok(mem) = mem_str.parse::<u64>() {
                            return mem;
                        }
                    }
                }
            }
        }
    }
    0
}

fn get_cpu_usage() -> f32 {
    // This would require platform-specific implementation
    // Returning a placeholder value
    0.0
}

// Helper function to get scan statistics
fn get_scan_statistics() -> serde_json::Value {
    let config_dir = crate::get_config_dir();
    let stats_path = config_dir.join("data").join("latest_stats.json");
    
    if let Ok(content) = std::fs::read_to_string(stats_path) {
        if let Ok(stats) = serde_json::from_str::<serde_json::Value>(&content) {
            return stats;
        }
    }
    
    // Return empty stats if no data available
    serde_json::json!({
        "timestamp": 0,
        "total_files": 0,
        "total_size": 0,
        "file_types": {}
    })
}

// Add a detailed file listing endpoint with pagination and filtering
#[derive(Deserialize)]
struct FileListRequest {
    page: Option<usize>,
    page_size: Option<usize>,
    sort_by: Option<String>,
    sort_order: Option<String>,
    filter_category: Option<String>,
    filter_size_min: Option<u64>,
    filter_size_max: Option<u64>,
    search_term: Option<String>,
}

async fn get_file_list(query: web::Query<FileListRequest>) -> impl Responder {
    let config_dir = crate::get_config_dir();
    
    match storage::load_file_metadata(&config_dir) {
        Ok(metadata) => {
            // Convert to a vector for easier filtering and sorting
            let mut files: Vec<&storage::FileMetadata> = metadata.values().collect();
            
            // Apply filters
            if let Some(category) = &query.filter_category {
                files.retain(|meta| meta.category.to_lowercase() == category.to_lowercase());
            }
            
            if let Some(min_size) = query.filter_size_min {
                files.retain(|meta| meta.size >= min_size);
            }
            
            if let Some(max_size) = query.filter_size_max {
                files.retain(|meta| meta.size <= max_size);
            }
            
            if let Some(term) = &query.search_term {
                let term_lower = term.to_lowercase();
                files.retain(|meta| {
                    meta.file_name.to_lowercase().contains(&term_lower) || 
                    meta.path.to_string_lossy().to_lowercase().contains(&term_lower)
                });
            }
            
            // Sort files
            let sort_by = query.sort_by.as_deref().unwrap_or("name");
            let ascending = query.sort_order.as_deref().unwrap_or("asc") == "asc";
            
            match sort_by {
                "name" => {
                    if ascending {
                        files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
                    } else {
                        files.sort_by(|a, b| b.file_name.cmp(&a.file_name));
                    }
                },
                "size" => {
                    if ascending {
                        files.sort_by(|a, b| a.size.cmp(&b.size));
                    } else {
                        files.sort_by(|a, b| b.size.cmp(&a.size));
                    }
                },
                "date" => {
                    if ascending {
                        files.sort_by(|a, b| a.modified.cmp(&b.modified));
                    } else {
                        files.sort_by(|a, b| b.modified.cmp(&a.modified));
                    }
                },
                "importance" => {
                    if ascending {
                        files.sort_by(|a, b| a.importance_score.cmp(&b.importance_score));
                    } else {
                        files.sort_by(|a, b| b.importance_score.cmp(&a.importance_score));
                    }
                },
                _ => {}
            }
            
            // Apply pagination
            let page = query.page.unwrap_or(1).max(1);
            let page_size = query.page_size.unwrap_or(50).min(1000);
            let total_files = files.len();
            let total_pages = (total_files + page_size - 1) / page_size;
            let start_index = (page - 1) * page_size;
            let end_index = (start_index + page_size).min(total_files);
            
            let page_files = if start_index < total_files {
                &files[start_index..end_index]
            } else {
                &[]
            };
            
            // Convert to a simplified format for the frontend
            let file_list: Vec<serde_json::Value> = page_files.iter()
                .map(|meta| {
                    serde_json::json!({
                        "id": meta.path.to_string_lossy().to_string(),
                        "path": meta.path.to_string_lossy().to_string(),
                        "name": meta.file_name,
                        "extension": meta.extension,
                        "size": meta.size,
                        "size_formatted": format_file_size(meta.size),
                        "created": meta.created.timestamp(),
                        "modified": meta.modified.timestamp(),
                        "category": meta.category,
                        "mime_type": meta.mime_type,
                        "importance": meta.importance_score,
                        "is_duplicate": meta.is_duplicate,
                    })
                })
                .collect();
            
            HttpResponse::Ok().json(serde_json::json!({
                "total": total_files,
                "page": page,
                "page_size": page_size,
                "total_pages": total_pages,
                "files": file_list
            }))
        },
        Err(_) => {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to load metadata"
            }))
        }
    }
}

// Helper function to format file size
fn format_file_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    
    if size < KB {
        format!("{} B", size)
    } else if size < MB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else if size < GB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else {
        format!("{:.1} GB", size as f64 / GB as f64)
    }
}

// Get detailed info about a specific file
async fn get_file_details(path: web::Path<String>) -> impl Responder {
    let file_path = std::path::Path::new(&*path);
    let config_dir = crate::get_config_dir();
    
    match storage::load_file_metadata(&config_dir) {
        Ok(metadata) => {
            // Find the file in the metadata
            for (_, meta) in metadata.iter() {
                if meta.path == file_path {
                    return HttpResponse::Ok().json(serde_json::json!({
                        "path": meta.path.to_string_lossy().to_string(),
                        "name": meta.file_name,
                        "extension": meta.extension,
                        "size": meta.size,
                        "size_formatted": format_file_size(meta.size),
                        "created": meta.created.timestamp(),
                        "modified": meta.modified.timestamp(),
                        "category": meta.category,
                        "mime_type": meta.mime_type,
                        "importance": meta.importance_score,
                        "is_duplicate": meta.is_duplicate,
                        "duplicate_of": meta.duplicate_of.as_ref().map(|p| p.to_string_lossy().to_string()),
                        "ai_analysis": meta.ai_analysis,
                    }));
                }
            }
            
            HttpResponse::NotFound().json(serde_json::json!({
                "error": "File not found in metadata"
            }))
        },
        Err(_) => {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to load metadata"
            }))
        }
    }
}

async fn create_file(data: web::Json<FileOpRequest>) -> impl Responder {
    let path = std::path::Path::new(&data.path);
    if let Some(content) = &data.content {
        if let Ok(mut file) = std::fs::File::create(path) {
            use std::io::Write;
            let _ = file.write_all(content.as_bytes());
            return HttpResponse::Ok().json(serde_json::json!({"status": "created"}));
        }
    } else if let Ok(_) = std::fs::File::create(path) {
        return HttpResponse::Ok().json(serde_json::json!({"status": "created"}));
    }
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to create file"}))
}

async fn delete_file(data: web::Json<FileOpRequest>) -> impl Responder {
    let path = std::path::Path::new(&data.path);
    if std::fs::remove_file(path).is_ok() {
        return HttpResponse::Ok().json(serde_json::json!({"status": "deleted"}));
    }
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to delete file"}))
}

async fn rename_file(data: web::Json<FileOpRequest>) -> impl Responder {
    if let Some(new_path) = &data.new_path {
        if std::fs::rename(&data.path, new_path).is_ok() {
            return HttpResponse::Ok().json(serde_json::json!({"status": "renamed"}));
        }
    }
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to rename file"}))
}

async fn copy_file(data: web::Json<FileOpRequest>) -> impl Responder {
    if let Some(new_path) = &data.new_path {
        if std::fs::copy(&data.path, new_path).is_ok() {
            return HttpResponse::Ok().json(serde_json::json!({"status": "copied"}));
        }
    }
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to copy file"}))
}

async fn move_file(data: web::Json<FileOpRequest>) -> impl Responder {
    if let Some(new_path) = &data.new_path {
        if std::fs::rename(&data.path, new_path).is_ok() {
            return HttpResponse::Ok().json(serde_json::json!({"status": "moved"}));
        }
    }
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to move file"}))
}

pub fn start_server(config: Arc<Mutex<Config>>, port: u16, verbose: bool) {
    if verbose {
        println!("Starting API server on http://0.0.0.0:{}", port);
    }
    
    // Use actix_web to run the server
    let config_data = web::Data::new(config);
    
    // Create an actix system
    let system = actix_web::rt::System::new();
    
    // Run the server in the system
    system.block_on(async move {
        // Configure CORS to allow access from Flutter app
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);
        
        let app = HttpServer::new(move || {
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);
                
            App::new()
                .wrap(Logger::default())
                .wrap(cors)
                .app_data(config_data.clone())
                .route("/health", web::get().to(health_check))
                .route("/status", web::get().to(get_system_status))
                .route("/drives", web::get().to(get_drives))
                .route("/stats", web::get().to(get_scan_stats))
                .route("/scan", web::post().to(initiate_scan))
                .route("/metadata", web::get().to(get_metadata))
                .route("/files", web::get().to(get_file_list))
                .route("/files/{path:.*}", web::get().to(get_file_details))
                .route("/config", web::get().to(get_config))
                .route("/config", web::post().to(update_config))
                .route("/file/create", web::post().to(create_file))
                .route("/file/delete", web::post().to(delete_file))
                .route("/file/rename", web::post().to(rename_file))
                .route("/file/copy", web::post().to(copy_file))
                .route("/file/move", web::post().to(move_file))
        })
        .bind(format!("0.0.0.0:{}", port))
        .expect(&format!("Failed to bind to port {}", port));
        
        if verbose {
            println!("API server started successfully on port {}", port);
        }
        
        // Spawn a task to listen for Ctrl+C and stop the system
        actix_web::rt::spawn(async {
            tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
            println!("Ctrl+C received. Shutting down server.");
            actix_web::rt::System::current().stop();
        });
        
        // Run the server; this will block until the system is stopped.
        app.run().await.expect("Failed to run server");
        // After shutdown, exit process to free terminal
        std::process::exit(0);
    });
}
