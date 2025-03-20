use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde_derive::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use std::fs;
use std::io;
use crate::ai_integration::AIAnalysisResult;
use crate::scanner::ScanResult;
use serde_json;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub file_name: String,
    pub extension: String,
    pub size: u64,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub created: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub modified: DateTime<Utc>,
    pub category: String,
    pub mime_type: String,
    pub importance_score: u8,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_accessed: DateTime<Utc>,
    pub is_duplicate: bool,
    pub duplicate_of: Option<PathBuf>,
    pub ai_analysis: Option<AIAnalysisResult>,
}

pub fn save_scan_result(config_dir: &Path, result: &ScanResult) -> io::Result<()> {
    // Create the data directory if it doesn't exist
    let data_dir = config_dir.join("data");
    fs::create_dir_all(&data_dir)?;
    
    // Save overall statistics
    let stats = serde_json::json!({
        "timestamp": Utc::now().timestamp(), // changed: using timestamp (i64)
        "total_files": result.total_files,
        "total_size": result.total_size,
        "file_types": result.file_types,
    });
    
    let stats_path = data_dir.join("latest_stats.json");
    fs::write(&stats_path, serde_json::to_string_pretty(&stats)?)?;
    
    // Save file metadata
    // Split into chunks to avoid large files
    let chunk_size = 10000;
    let mut current_chunk = 0;
    let mut current_index = 0;
    
    for (path, metadata) in &result.metadata {
        if current_index % chunk_size == 0 {
            current_chunk += 1;
        }
        
        let chunk_path = data_dir.join(format!("metadata_chunk_{}.json", current_chunk));
        
        // Load existing chunk if it exists
        let mut chunk_data: HashMap<String, FileMetadata> = if chunk_path.exists() {
            match fs::read_to_string(&chunk_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };
        
        // Add metadata to chunk
        chunk_data.insert(path.to_string_lossy().to_string(), metadata.clone());
        
        // Save chunk
        fs::write(&chunk_path, serde_json::to_string_pretty(&chunk_data)?)?;
        
        current_index += 1;
    }
    
    Ok(())
}

pub fn load_file_metadata(config_dir: &Path) -> io::Result<HashMap<PathBuf, FileMetadata>> {
    let data_dir = config_dir.join("data");
    let mut result = HashMap::new();
    
    if !data_dir.exists() {
        return Ok(result);
    }
    
    // Find all metadata chunk files
    for entry in fs::read_dir(&data_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.file_name().unwrap().to_string_lossy().starts_with("metadata_chunk_") {
            let content = fs::read_to_string(&path)?;
            let chunk_data: HashMap<String, FileMetadata> = serde_json::from_str(&content)?;
            
            // Convert string keys to PathBuf and add to result
            for (_, metadata) in chunk_data {
                result.insert(metadata.path.clone(), metadata);
            }
        }
    }
    
    Ok(result)
}

pub fn find_duplicate_files(metadata: &HashMap<PathBuf, FileMetadata>) -> Vec<(PathBuf, PathBuf)> {
    let mut size_map: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut duplicates = Vec::new();
    
    // Group files by size
    for (path, meta) in metadata {
        size_map.entry(meta.size).or_default().push(path.clone());
    }
    
    // For each size group with multiple files, check if they are duplicates
    for (_, files) in size_map.into_iter().filter(|(_, files)| files.len() > 1) {
        // Compare files by content hash
        for i in 0..files.len() {
            for j in i + 1..files.len() {
                if are_files_identical(&files[i], &files[j]) {
                    duplicates.push((files[i].clone(), files[j].clone()));
                }
            }
        }
    }
    
    duplicates
}

fn are_files_identical(path1: &Path, path2: &Path) -> bool {
    // Simple implementation: read and compare file contents
    // For production, you'd want to use hashing or more efficient methods
    match (fs::read(path1), fs::read(path2)) {
        (Ok(content1), Ok(content2)) => content1 == content2,
        _ => false,
    }
}
