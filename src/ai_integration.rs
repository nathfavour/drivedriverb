use std::path::Path;
use serde_derive::{Serialize, Deserialize};
use reqwest::blocking::Client;
use std::io::Read;
use serde_json;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AIAnalysisResult {
    pub file_purpose: String,
    pub importance_level: String,
    pub potential_category: String,
    pub deletion_recommendation: bool,
    pub confidence_score: f32,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

pub fn analyze_file_with_ai(path: &Path, config: &crate::config::Config) -> Option<AIAnalysisResult> {
    // Only analyze if AI analysis is enabled
    if !config.use_ai_analysis {
        return None;
    }
    
    // Only analyze text-based files that are within a reasonable size limit
    if !is_analyzable(path) {
        return None;
    }
    
    // Read the first portion of the file
    let file_sample = read_file_sample(path, 4096)?;
    
    // Get the file name and extension
    let file_name = path.file_name()?.to_string_lossy();
    let extension = path.extension().map(|ext| ext.to_string_lossy()).unwrap_or_default();
    
    // Create prompt for Ollama
    let prompt = format!(
        "Analyze this file sample. File name: {}, Extension: {}\n\nSample content:\n{}\n\n\
        Please provide a JSON response with the following fields:\n\
        - file_purpose: What is the likely purpose of this file?\n\
        - importance_level: Estimate importance (low, medium, high)\n\
        - potential_category: Best category for this file\n\
        - deletion_recommendation: Boolean if this seems like a temporary or unnecessary file\n\
        - confidence_score: Your confidence in this analysis from 0.0 to 1.0",
        file_name, extension, file_sample
    );
    
    // Send request to Ollama API
    let client = Client::new();
    let ollama_request = OllamaRequest {
        model: config.ollama_model.clone(),
        prompt,
    };
    
    match client.post(&config.ollama_url)
        .json(&ollama_request)
        .send() {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(ollama_response) = response.json::<OllamaResponse>() {
                    // Parse JSON response
                    if let Ok(analysis) = serde_json::from_str::<AIAnalysisResult>(&ollama_response.response) {
                        return Some(analysis);
                    }
                }
            }
        },
        Err(e) => {
            println!("Error communicating with Ollama: {}", e);
        }
    }
    
    None
}

fn is_analyzable(path: &Path) -> bool {
    if let Ok(metadata) = std::fs::metadata(path) {
        // Only analyze files smaller than 1MB
        if metadata.len() > 1_000_000 {
            return false;
        }
        
        // Check if it's a text-based file by extension
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            match ext.as_str() {
                "txt" | "md" | "json" | "csv" | "xml" | "html" | "htm" | "css" | "js" |
                "py" | "rs" | "java" | "c" | "cpp" | "h" | "hpp" | "sh" | "bat" | "ps1" |
                "log" | "conf" | "ini" | "yaml" | "yml" | "toml" => return true,
                _ => {}
            }
        }
    }
    
    false
}

fn read_file_sample(path: &Path, max_bytes: usize) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0; max_bytes];
    let bytes_read = file.read(&mut buffer).ok()?;
    buffer.truncate(bytes_read);
    
    // Try to convert to UTF-8 string
    String::from_utf8(buffer).ok()
}
