use std::path::Path;
use std::fs::Metadata;
use crate::storage::FileMetadata;
use crate::ai_integration;
use std::time::SystemTime;
use chrono::{DateTime, Utc};

pub fn analyze_file(path: &Path, system_metadata: &Metadata) -> FileMetadata {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let extension = path.extension().map(|ext| ext.to_string_lossy().to_lowercase());
    let file_size = system_metadata.len();
    
    // Get creation and modification times
    let created = system_time_to_date_time(system_metadata.created().unwrap_or(SystemTime::now()));
    let modified = system_time_to_date_time(system_metadata.modified().unwrap_or(SystemTime::now()));
    
    // Determine file category
    let category = determine_file_category(path, &extension);
    
    // Create basic metadata
    let mut metadata = FileMetadata {
        path: path.to_path_buf(),
        file_name,
        extension: extension.unwrap_or_default(),
        size: file_size,
        created,
        modified,
        category,
        mime_type: get_mime_type(path),
        importance_score: 0,
        last_accessed: modified,
        is_duplicate: false,
        duplicate_of: None,
        ai_analysis: None,
    };
    
    // Set importance score based on initial analysis
    metadata.importance_score = calculate_importance_score(&metadata);
    
    metadata
}

fn determine_file_category(path: &Path, extension: &Option<String>) -> String {
    if let Some(ext) = extension {
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "heic" => "image".to_string(),
            "mp4" | "avi" | "mov" | "wmv" | "flv" | "mkv" | "webm" => "video".to_string(),
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => "audio".to_string(),
            "doc" | "docx" | "pdf" | "txt" | "rtf" | "odt" => "document".to_string(),
            "xls" | "xlsx" | "csv" | "ods" => "spreadsheet".to_string(),
            "ppt" | "pptx" | "odp" => "presentation".to_string(),
            "exe" | "app" | "dmg" | "deb" | "rpm" => "application".to_string(),
            "zip" | "rar" | "7z" | "tar" | "gz" => "archive".to_string(),
            _ => "other".to_string()
        }
    } else {
        // Files without extensions
        if is_executable(path) {
            "application".to_string()
        } else {
            "other".to_string()
        }
    }
}

fn calculate_importance_score(metadata: &FileMetadata) -> u8 {
    let mut score = 0;
    
    // Recently modified files are more important
    let now = Utc::now();
    let days_since_modification = (now - metadata.modified).num_days();
    
    if days_since_modification < 7 {
        score += 30;
    } else if days_since_modification < 30 {
        score += 20;
    } else if days_since_modification < 90 {
        score += 10;
    }
    
    // File category importance
    match metadata.category.as_str() {
        "document" => score += 25,
        "spreadsheet" | "presentation" => score += 20,
        "image" | "video" => score += 15,
        "audio" => score += 10,
        "application" => score += 5,
        _ => score += 0,
    }
    
    // Clamp to 0-100
    score.min(100) as u8
}

fn get_mime_type(path: &Path) -> String {
    // Simple MIME type detection based on extension
    if let Some(ext) = path.extension() {
        match ext.to_string_lossy().to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            // Add more mappings as needed
            _ => "application/octet-stream",
        }
        .to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            return metadata.permissions().mode() & 0o111 != 0;
        }
    }
    
    #[cfg(windows)]
    {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            return ext == "exe" || ext == "bat" || ext == "cmd";
        }
    }
    
    false
}

fn system_time_to_date_time(time: SystemTime) -> DateTime<Utc> {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    let secs = duration.as_secs() as i64;
    let nsecs = duration.subsec_nanos() as u32;
    DateTime::from_timestamp(secs, nsecs).unwrap_or(Utc::now())
}
