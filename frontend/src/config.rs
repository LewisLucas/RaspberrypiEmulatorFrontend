use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CmdTemplate {
    pub program: String,
    pub args: Vec<String>,
    pub extensions: Option<Vec<String>>,
    pub visible_extensions: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigFile {
    pub default: Option<CmdTemplate>,
    pub systems: Option<HashMap<String, CmdTemplate>>,
    pub show_empty_systems: Option<bool>,
    pub controller_map: Option<HashMap<String, String>>,
    pub default_roms_path: Option<String>,
    pub font_path: Option<String>,
}

pub fn user_config_path() -> Option<std::path::PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("rpi_emulator_frontend");
        p.push("config.toml");
        Some(p)
    } else if let Some(home) = dirs::home_dir() {
        let mut p = home;
        p.push(".config/rpi_emulator_frontend/config.toml");
        Some(p)
    } else {
        None
    }
}

fn write_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sample = if let Ok(template) = std::fs::read_to_string("config_template.toml") {
        template
    } else {
        include_str!("../config.sample.toml").to_string()
    };
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, sample.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_config() -> ConfigFile {
    let mut cfg = ConfigFile {
        default: Some(CmdTemplate {
            program: "mgba-qt".to_string(),
            args: vec!["{rom}".to_string()],
            extensions: None,
            visible_extensions: None,
        }),
        systems: None,
        show_empty_systems: Some(false),
        controller_map: None,
        default_roms_path: None,
        font_path: None,
    };
    if let Some(p) = user_config_path() {
        if !p.exists() {
            if let Err(e) = write_default_config(&p) {
                eprintln!("Failed to write default config: {}", e);
            }
        }
        if let Ok(contents) = std::fs::read_to_string(&p) {
            if let Ok(parsed) = toml::from_str::<ConfigFile>(&contents) {
                if parsed.default.is_some() {
                    cfg.default = parsed.default;
                }
                if parsed.systems.is_some() {
                    cfg.systems = parsed.systems;
                }
                if parsed.show_empty_systems.is_some() {
                    cfg.show_empty_systems = parsed.show_empty_systems;
                }
                if parsed.controller_map.is_some() {
                    cfg.controller_map = parsed.controller_map;
                }
                if parsed.default_roms_path.is_some() {
                    cfg.default_roms_path = parsed.default_roms_path;
                }
                if parsed.font_path.is_some() {
                    cfg.font_path = parsed.font_path;
                }
            } else {
                eprintln!("Failed to parse config at {}", p.display());
            }
        }
    }
    cfg
}

pub fn write_config(cfg: &ConfigFile) -> Result<(), String> {
    if let Some(p) = user_config_path() {
        if let Some(parent) = p.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(format!("Failed to create config dir: {}", e));
            }
        }
        match toml::to_string_pretty(cfg) {
            Ok(s) => {
                let tmp = p.with_extension("toml.tmp");
                if let Err(e) = std::fs::write(&tmp, s.as_bytes()) {
                    return Err(format!("Failed writing tmp config: {}", e));
                }
                if let Err(e) = std::fs::rename(&tmp, &p) {
                    return Err(format!("Failed renaming config: {}", e));
                }
                return Ok(());
            }
            Err(e) => return Err(format!("Failed to serialize config: {}", e)),
        }
    }
    Err("No config path available".into())
}
