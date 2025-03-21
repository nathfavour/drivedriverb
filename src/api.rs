use std::sync::{Arc, Mutex};
use actix_web::{web, App, HttpResponse, HttpServer, Responder, middleware::Logger};
use actix_cors::Cors;
use serde_derive::{Serialize, Deserialize};
use crate::config::Config;
use crate::scanner;
use crate::storage;
use std::thread;
use std::path::PathBuf;

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
            // Converting to a simplified format for the frontend
            let simplified: Vec<serde_json::Value> = metadata.values()
                .take(1000) // Limit to 1000 entries to avoid overwhelming the frontend
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
            
            HttpResponse::Ok().json(simplified)
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
                .route("/drives", web::get().to(get_drives))
                .route("/stats", web::get().to(get_scan_stats))
                .route("/scan", web::post().to(initiate_scan))
                .route("/metadata", web::get().to(get_metadata))
        })
        .bind("0.0.0.0:8080")
        .expect("Failed to bind to port 8080");
        
        println!("API server started successfully");
        
        // Run the server
        app.run().await.expect("Failed to run server");
    });
}
