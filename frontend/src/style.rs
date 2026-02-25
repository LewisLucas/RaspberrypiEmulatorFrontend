use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StyleConfig {
    pub background: Option<[u8; 3]>,
    pub tile_selected: Option<[u8; 3]>,
    pub tile_normal: Option<[u8; 3]>,
    pub text_primary: Option<[u8; 3]>,
    pub text_secondary: Option<[u8; 3]>,
    pub banner_bg: Option<[u8; 3]>,
    pub banner_text: Option<[u8; 3]>,
    pub emu_text: Option<[u8; 3]>,
    pub overlay_bg: Option<[u8; 3]>,
    pub overlay_alpha: Option<u8>,
    pub menu_bg: Option<[u8; 3]>,
    pub menu_box: Option<[u8; 3]>,
    pub menu_selected: Option<[u8; 3]>,
    pub menu_title: Option<[u8; 3]>,
    pub menu_text: Option<[u8; 3]>,
    pub error_overlay_alpha: Option<u8>,
    pub message_overlay_alpha: Option<u8>,
}

pub fn user_style_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("rpi_emulator_frontend");
        p.push("style.toml");
        Some(p)
    } else if let Some(home) = dirs::home_dir() {
        let mut p = home;
        p.push(".config/rpi_emulator_frontend/style.toml");
        Some(p)
    } else {
        None
    }
}

fn write_default_style(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sample = if let Ok(s) = std::fs::read_to_string("style.sample.toml") {
        s
    } else {
        include_str!("../style.sample.toml").to_string()
    };
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, sample.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_style() -> StyleConfig {
    let mut s = StyleConfig {
        background: Some([12, 12, 12]),
        tile_selected: Some([200, 180, 50]),
        tile_normal: Some([60, 60, 60]),
        text_primary: Some([240, 240, 240]),
        text_secondary: Some([180, 180, 180]),
        banner_bg: Some([20, 20, 20]),
        banner_text: Some([220, 220, 220]),
        emu_text: Some([180, 180, 180]),
        overlay_bg: Some([0, 0, 0]),
        overlay_alpha: Some(200),
        menu_bg: Some([10, 10, 10]),
        menu_box: Some([40, 40, 40]),
        menu_selected: Some([80, 80, 80]),
        menu_title: Some([230, 230, 230]),
        menu_text: Some([220, 220, 220]),
        error_overlay_alpha: Some(200),
        message_overlay_alpha: Some(160),
    };

    if let Some(p) = user_style_path() {
        if !p.exists() {
            if let Err(e) = write_default_style(&p) {
                eprintln!("Failed to write default style: {}", e);
            }
        }
        if let Ok(contents) = std::fs::read_to_string(&p) {
            if let Ok(parsed) = toml::from_str::<StyleConfig>(&contents) {
                if parsed.background.is_some() {
                    s.background = parsed.background;
                }
                if parsed.tile_selected.is_some() {
                    s.tile_selected = parsed.tile_selected;
                }
                if parsed.tile_normal.is_some() {
                    s.tile_normal = parsed.tile_normal;
                }
                if parsed.text_primary.is_some() {
                    s.text_primary = parsed.text_primary;
                }
                if parsed.text_secondary.is_some() {
                    s.text_secondary = parsed.text_secondary;
                }
                if parsed.banner_bg.is_some() {
                    s.banner_bg = parsed.banner_bg;
                }
                if parsed.banner_text.is_some() {
                    s.banner_text = parsed.banner_text;
                }
                if parsed.emu_text.is_some() {
                    s.emu_text = parsed.emu_text;
                }
                if parsed.overlay_bg.is_some() {
                    s.overlay_bg = parsed.overlay_bg;
                }
                if parsed.overlay_alpha.is_some() {
                    s.overlay_alpha = parsed.overlay_alpha;
                }
                if parsed.menu_bg.is_some() {
                    s.menu_bg = parsed.menu_bg;
                }
                if parsed.menu_box.is_some() {
                    s.menu_box = parsed.menu_box;
                }
                if parsed.menu_selected.is_some() {
                    s.menu_selected = parsed.menu_selected;
                }
                if parsed.menu_title.is_some() {
                    s.menu_title = parsed.menu_title;
                }
                if parsed.menu_text.is_some() {
                    s.menu_text = parsed.menu_text;
                }
                if parsed.error_overlay_alpha.is_some() {
                    s.error_overlay_alpha = parsed.error_overlay_alpha;
                }
                if parsed.message_overlay_alpha.is_some() {
                    s.message_overlay_alpha = parsed.message_overlay_alpha;
                }
            } else {
                eprintln!("Failed to parse style at {}", p.display());
            }
        }
    }

    s
}
