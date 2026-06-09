#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SavedColors {
    pub background: [u8; 4],
    pub waveform: [u8; 4],
    pub playhead: [u8; 4],
    pub selection_fill: [u8; 4],
    pub selection_edge: [u8; 4],
    pub center_line: [u8; 4],
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ColorScheme {
    pub name: String,
    pub colors: SavedColors,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct AppConfig {
    pub active_colors: Option<SavedColors>,
    pub saved_schemes: Vec<ColorScheme>,
}

pub fn built_in_schemes() -> Vec<ColorScheme> {
    vec![
        ColorScheme {
            name: "Dark Green".to_string(),
            colors: SavedColors {
                background: [18, 18, 28, 255],
                waveform: [80, 200, 120, 255],
                playhead: [255, 255, 255, 255],
                selection_fill: [80, 140, 255, 45],
                selection_edge: [100, 160, 255, 255],
                center_line: [40, 40, 60, 255],
            },
        },
        ColorScheme {
            name: "Ocean Blue".to_string(),
            colors: SavedColors {
                background: [10, 18, 35, 255],
                waveform: [60, 160, 230, 255],
                playhead: [200, 220, 255, 255],
                selection_fill: [100, 200, 255, 40],
                selection_edge: [120, 210, 255, 255],
                center_line: [20, 35, 60, 255],
            },
        },
    ]
}

fn config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var("APPDATA").ok().map(std::path::PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        });
    base.map(|b| b.join("audio-manipulator").join("config.json"))
}

pub fn load() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return AppConfig::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save(config: &AppConfig) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
    }
}
