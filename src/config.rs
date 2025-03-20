use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Config {
    pub use_ai_analysis: bool,
    pub ollama_model: String,
    pub ollama_url: String,
    pub excluded_paths: HashSet<PathBuf>,
}

impl Config {
    pub fn load_or_create(config_path: &Path) -> Self {
        if !config_path.exists() {
            let default = Config {
                use_ai_analysis: false,
                ollama_model: "default-model".to_string(),
                ollama_url: "http://localhost:11434".to_string(),
                excluded_paths: HashSet::new(),
            };
            // Optionally save default configuration
            return default;
        }
        // Load configuration logic (placeholder)
        Config {
            use_ai_analysis: true,
            ollama_model: "default-model".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            excluded_paths: HashSet::new(),
        }
    }

    pub fn is_path_excluded(&self, path: &Path) -> bool {
        self.excluded_paths.contains(path)
    }
}
