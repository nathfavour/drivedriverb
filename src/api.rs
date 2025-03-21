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

async fn health_check() -> impl Responder {
    let response = StatusResponse {
        status: "running".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    HttpResponse::Ok().json(response)
}

async fn get_drives() -> impl Responder {
    let drives = scanner::get_all_drives();
    let drive_paths: Vec<String> = drives.iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    
    HttpResponse::Ok().json(DriveInfoResponse { drives: drive_paths })
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
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to load metadata"
            }))
        }
    }
}

// Add a new endpoint for system status that includes more details
async fn get_system_status() -> impl Responder {
    let response = serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "memory_usage": get_memory_usage(),
        "cpu_usage": get_cpu_usage(),
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

pub fn start_server(config: Arc<Mutex<Config>>) {
    println!("Starting API server on http://0.0.0.0:8080");
    
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
        })
        .bind("0.0.0.0:8080")
        .expect("Failed to bind to port 8080");
        
        println!("API server started successfully");
        
        // Spawn a task to listen for Ctrl+C and stop the system
        actix_web::rt::spawn(async {
            tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
            println!("Ctrl+C received. Shutting down server.");
            actix_web::rt::System::current().stop();
        });
        
        // Run the server; this will block until the system is stopped.
        app.run().await.expect("Failed to run server");
    });
}
