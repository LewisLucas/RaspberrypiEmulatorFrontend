use sdl2::event::Event;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Texture, TextureCreator, WindowCanvas};
use sdl2::ttf::Font;
use sdl2::video::WindowContext;
use sdl2::EventPump;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(PartialEq)]
pub enum MenuState {
    Closed,
    Open {
        items: Vec<String>,
        selected: usize,
    },
    Remap {
        actions: Vec<String>,
        idx: usize,
        temp_map: HashMap<String, String>,
    },
}

pub struct UIColors {
    pub bg: Color,
    pub tile_selected: Color,
    pub tile_normal: Color,
    pub text_primary: Color,
    pub banner_bg: Color,
    pub banner_text: Color,
    pub emu_text: Color,
    pub overlay_rgba: Color,
}

/// Render the main list, banner and simple overlays (launching / error)
pub fn render_frame<'a>(
    canvas: &mut WindowCanvas,
    texture_creator: &'a TextureCreator<WindowContext>,
    font: &Font,
    colors: &UIColors,
    current_roms: &Vec<PathBuf>,
    text_textures: &mut Vec<Option<Vec<Texture<'a>>>>,
    selected: usize,
    scroll_offset: usize,
    current_system_idx: usize,
    systems_vec: &Vec<String>,
    w: i32,
    h: i32,
    launching: bool,
    error_overlay: &mut Option<(String, Instant)>,
) {
    // clear
    canvas.set_draw_color(colors.bg);
    canvas.clear();

    let padding = 10;
    let start_x = padding;
    let start_y = padding + 44; // leave space for banner
    let tile_w = (w as i32) - (padding * 2);
    let tile_h = super::TILE_H;

    let available_h = (h as i32) - start_y - padding;
    let visible = (available_h / (tile_h + padding)).max(1) as usize;

    // ensure scroll offset valid
    let mut scroll_offset = scroll_offset;
    if scroll_offset >= current_roms.len() && !current_roms.is_empty() {
        scroll_offset = current_roms.len() - 1;
    }

    // render list tiles
    let text_primary_c = colors.text_primary;
    for (idx, rom) in current_roms
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible)
    {
        let i = idx;
        let x = start_x;
        let y = start_y + ((i - scroll_offset) as i32) * (tile_h + padding);
        let rect = Rect::new(x, y, tile_w as u32, tile_h as u32);

        if i == selected {
            canvas.set_draw_color(colors.tile_selected);
        } else {
            canvas.set_draw_color(colors.tile_normal);
        }
        let _ = canvas.fill_rect(rect);

        // filename text rendering (lazy create texture)
        if text_textures.get(i).and_then(|t| t.as_ref()).is_none() {
            if let Some(name) = rom.file_name().and_then(|s| s.to_str()) {
                let padding = 8;
                let max_w = (tile_w as u32).saturating_sub((padding * 2) as u32);
                let width_of = |s: &str| -> u32 { font.size_of(s).map(|(w, _)| w).unwrap_or(0) };
                if width_of(name) <= max_w {
                    if let Ok(surface) = font.render(name).blended(text_primary_c) {
                        if let Ok(tex) = texture_creator.create_texture_from_surface(&surface) {
                            if let Some(slot) = text_textures.get_mut(i) {
                                *slot = Some(vec![tex]);
                            }
                        }
                    }
                } else {
                    let chars: Vec<char> = name.chars().collect();
                    let mut lo = 0usize;
                    let mut hi = chars.len();
                    while lo < hi {
                        let mid = (lo + hi + 1) / 2;
                        let cand: String = chars.iter().take(mid).collect();
                        if width_of(&cand) <= max_w {
                            lo = mid;
                        } else {
                            hi = mid - 1;
                        }
                    }
                    let mut first: String = chars.iter().take(lo).collect();
                    let remaining: String = chars.iter().skip(lo).collect();
                    let seps = [' ', '-', ':', '_'];
                    if let Some(pos) = first.rfind(|c: char| seps.contains(&c)) {
                        let new_first: String = first.chars().take(pos).collect();
                        if !new_first.is_empty() {
                            let after_sep: String =
                                first.chars().skip(pos + 1).collect::<String>() + &remaining;
                            first = new_first;
                            let remaining = after_sep;
                            let second = if width_of(&remaining) <= max_w {
                                remaining
                            } else {
                                let ell = "...";
                                let mut lo2 = 0usize;
                                let mut hi2 = remaining.chars().count();
                                while lo2 < hi2 {
                                    let mid = (lo2 + hi2 + 1) / 2;
                                    let cand: String =
                                        remaining.chars().take(mid).collect::<String>() + ell;
                                    if width_of(&cand) <= max_w {
                                        lo2 = mid;
                                    } else {
                                        hi2 = mid - 1;
                                    }
                                }
                                let kept: String = remaining.chars().take(lo2).collect();
                                if kept.is_empty() {
                                    ell.to_string()
                                } else {
                                    kept + ell
                                }
                            };

                            let mut line_texts: Vec<Texture<'a>> = Vec::new();
                            if let Ok(s1) = font.render(&first).blended(text_primary_c) {
                                if let Ok(t1) = texture_creator.create_texture_from_surface(&s1) {
                                    line_texts.push(t1);
                                }
                            }
                            if let Ok(s2) = font.render(&second).blended(text_primary_c) {
                                if let Ok(t2) = texture_creator.create_texture_from_surface(&s2) {
                                    line_texts.push(t2);
                                }
                            }
                            if let Some(slot) = text_textures.get_mut(i) {
                                *slot = Some(line_texts);
                            }
                            continue;
                        }
                    }

                    let second = if width_of(&remaining) <= max_w {
                        remaining.clone()
                    } else {
                        let ell = "...";
                        let mut lo2 = 0usize;
                        let mut hi2 = remaining.chars().count();
                        while lo2 < hi2 {
                            let mid = (lo2 + hi2 + 1) / 2;
                            let cand: String =
                                remaining.chars().take(mid).collect::<String>() + ell;
                            if width_of(&cand) <= max_w {
                                lo2 = mid;
                            } else {
                                hi2 = mid - 1;
                            }
                        }
                        let kept: String = remaining.chars().take(lo2).collect();
                        if kept.is_empty() {
                            ell.to_string()
                        } else {
                            kept + ell
                        }
                    };

                    let mut line_texts: Vec<Texture> = Vec::new();
                    if let Ok(s1) = font.render(&first).blended(text_primary_c) {
                        if let Ok(t1) = texture_creator.create_texture_from_surface(&s1) {
                            line_texts.push(t1);
                        }
                    }
                    if let Ok(s2) = font.render(&second).blended(text_primary_c) {
                        if let Ok(t2) = texture_creator.create_texture_from_surface(&s2) {
                            line_texts.push(t2);
                        }
                    }
                    if let Some(slot) = text_textures.get_mut(i) {
                        *slot = Some(line_texts);
                    }
                }
            }
        }

        if let Some(Some(text_vec)) = text_textures.get(i) {
            let mut total_h = 0i32;
            let mut queries: Vec<sdl2::render::TextureQuery> = Vec::new();
            for tex in text_vec.iter() {
                let q = tex.query();
                total_h += q.height as i32;
                queries.push(q);
            }
            let spacing = 2;
            total_h += spacing * ((queries.len() as i32) - 1).max(0);
            let mut cursor_y = y + (tile_h - total_h) / 2;
            for (idx, tex) in text_vec.iter().enumerate() {
                let q = &queries[idx];
                let tex_w = q.width as i32;
                let tex_h = q.height as i32;
                let dst_x = x + (tile_w - tex_w) / 2;
                let dst_y = cursor_y;
                let _ = canvas.copy(
                    tex,
                    None,
                    Rect::new(dst_x, dst_y, tex_w as u32, tex_h as u32),
                );
                cursor_y += tex_h + spacing;
            }
        }
    }

    // banner
    canvas.set_draw_color(colors.banner_bg);
    let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, 40));

    let current_system_name = systems_vec
        .get(current_system_idx)
        .cloned()
        .unwrap_or_else(|| "".to_string());
    let count = current_roms.len();
    let system_label = format!("{} ({})", current_system_name.to_uppercase(), count);
    if let Ok(surf_sys) = font.render(&system_label).blended(colors.banner_text) {
        if let Ok(tex_sys) = texture_creator.create_texture_from_surface(&surf_sys) {
            let q = tex_sys.query();
            let dst_x = (w as i32) - (q.width as i32) - 12;
            let dst_y = 8;
            let _ = canvas.copy(&tex_sys, None, Rect::new(dst_x, dst_y, q.width, q.height));
        }
    }

    // display selected filename and emu similar to main
    if let Some(rom_path) = current_roms.get(selected) {
        if let Some(name) = rom_path.file_name().and_then(|s| s.to_str()) {
            let emu_name = None::<String>; // main still determines emulator mapping
            let banner_padding = 12u32;
            let avail = (w as u32).saturating_sub(banner_padding * 2);
            let full_name = name.to_string();
            let display_name = if font.size_of(&full_name).map(|(w, _)| w).unwrap_or(0) <= avail {
                full_name.clone()
            } else {
                fn elide_middle(s: &str, max_chars: usize) -> String {
                    let chars: Vec<char> = s.chars().collect();
                    if chars.len() <= max_chars {
                        return s.to_string();
                    }
                    if max_chars <= 3 {
                        return "...".to_string();
                    }
                    let keep = (max_chars - 3) / 2;
                    let head = keep + ((max_chars - 3) % 2);
                    let tail = keep;
                    let start: String = chars.iter().take(head).collect();
                    let end: String = chars
                        .iter()
                        .rev()
                        .take(tail)
                        .collect::<Vec<&char>>()
                        .into_iter()
                        .rev()
                        .collect();
                    format!("{}...{}", start, end)
                }
                let est = ((avail as f32) / 7.0) as usize;
                elide_middle(&full_name, est.max(8))
            };

            if let Ok(surf) = font.render(&display_name).blended(colors.banner_text) {
                if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                    let q = tex.query();
                    let dst_x = ((w as i32) - q.width as i32) / 2;
                    let dst_y = 8;
                    let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                }
            }

            if let Some(emu) = emu_name {
                let emu_txt = format!("emu: {}", emu);
                if let Ok(surf2) = font.render(&emu_txt).blended(colors.emu_text) {
                    if let Ok(tex2) = texture_creator.create_texture_from_surface(&surf2) {
                        let q2 = tex2.query();
                        let dst_x2 = 12;
                        let dst_y2 = 10;
                        let _ = canvas.copy(
                            &tex2,
                            None,
                            Rect::new(dst_x2, dst_y2, q2.width, q2.height),
                        );
                    }
                }
            }
        }
    }

    // launching overlay
    if launching {
        canvas.set_draw_color(colors.overlay_rgba);
        let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
    }

    // error overlay handling (auto-hide after 3s)
    if let Some((ref msg, when)) = error_overlay {
        if when.elapsed().as_secs() < 3 {
            canvas.set_draw_color(colors.overlay_rgba);
            let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
            if let Ok(surface) = font.render(msg).blended(colors.text_primary) {
                if let Ok(tex) = texture_creator.create_texture_from_surface(&surface) {
                    let q = tex.query();
                    let dst_x = (w as i32 - q.width as i32) / 2;
                    let dst_y = (h as i32 - q.height as i32) / 2;
                    let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                }
            }
        } else {
            *error_overlay = None;
        }
    }
}

/// Render and process menu overlay. Returns (next_state, optional_message, should_quit).
pub fn process_menu<'a>(
    canvas: &mut WindowCanvas,
    texture_creator: &'a TextureCreator<WindowContext>,
    font: &Font,
    menu_state: &mut MenuState,
    menu_events: &mut Vec<Event>,
    config: &mut crate::config::ConfigFile,
    groups: &mut HashMap<String, Vec<PathBuf>>,
    systems_vec: &mut Vec<String>,
    current_system_idx: &mut usize,
    roms_dir: &str,
    current_roms: &mut Vec<PathBuf>,
    text_textures: &mut Vec<Option<Vec<Texture<'a>>>>,
    event_pump: &mut EventPump,
) -> (Option<MenuState>, Option<(String, Instant)>, bool) {
    use crate::config::{load_config, write_config};
    let mut menu_next_state: Option<MenuState> = None;
    let mut menu_message: Option<(String, Instant)> = None;
    let mut should_quit = false;

    match menu_state {
        MenuState::Closed => {}
        MenuState::Open {
            items,
            selected: msel,
        } => {
            // draw overlay
            let menu_bg_c = Color::RGB(10, 10, 10);
            let menu_box_c = Color::RGB(40, 40, 40);
            let menu_selected_c = Color::RGB(80, 80, 80);
            let menu_text_c = Color::RGB(220, 220, 220);
            let menu_title_c = Color::RGB(230, 230, 230);
            canvas.set_draw_color(menu_bg_c);
            let _ = canvas.fill_rect(Rect::new(
                0,
                0,
                canvas.output_size().unwrap_or((800, 600)).0,
                canvas.output_size().unwrap_or((800, 600)).1,
            ));

            let (w, h) = canvas.output_size().unwrap_or((800, 600));
            let box_w = (w as i32) / 2;
            let box_h = (items.len() as i32) * 28 + 40;
            let box_x = (w as i32 - box_w) / 2;
            let box_y = (h as i32 - box_h) / 2;
            canvas.set_draw_color(menu_box_c);
            let _ = canvas.fill_rect(Rect::new(box_x, box_y, box_w as u32, box_h as u32));

            if let Ok(surf) = font.render("MENU").blended(menu_text_c) {
                if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                    let q = tex.query();
                    let bx = box_x + 12;
                    let by = box_y + 8;
                    let _ = canvas.copy(&tex, None, Rect::new(bx, by, q.width, q.height));
                }
            }

            if let Ok(surf) = font.render("Settings").blended(menu_title_c) {
                if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                    let q = tex.query();
                    let _ = canvas.copy(
                        &tex,
                        None,
                        Rect::new(box_x + 12, box_y + 8, q.width, q.height),
                    );
                }
            }

            for (i, it) in items.iter().enumerate() {
                let y = box_y + 40 + (i as i32) * 28;
                if i == *msel {
                    canvas.set_draw_color(menu_selected_c);
                    let _ = canvas.fill_rect(Rect::new(box_x + 8, y - 4, (box_w - 16) as u32, 28));
                }
                let label = if it == "Toggle show_empty_systems" {
                    let val = config.show_empty_systems.unwrap_or(false);
                    format!("{}: {}", it, if val { "ON" } else { "OFF" })
                } else {
                    it.clone()
                };
                if let Ok(surf) = font.render(&label).blended(menu_text_c) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                        let q = tex.query();
                        let _ =
                            canvas.copy(&tex, None, Rect::new(box_x + 16, y, q.width, q.height));
                    }
                }
            }

            // process events collected
            for event in menu_events.drain(..) {
                match event {
                    Event::KeyDown {
                        keycode: Some(k), ..
                    } => match k {
                        sdl2::keyboard::Keycode::Up => {
                            if *msel > 0 {
                                *msel -= 1;
                            }
                        }
                        sdl2::keyboard::Keycode::Down => {
                            if *msel + 1 < items.len() {
                                *msel += 1
                            }
                        }
                        sdl2::keyboard::Keycode::Return => {
                            let sel_label = items[*msel].as_str();
                            match sel_label {
                                "Toggle show_empty_systems" => {
                                    let cur = config.show_empty_systems.unwrap_or(false);
                                    config.show_empty_systems = Some(!cur);
                                    menu_message = Some((
                                        format!("show_empty_systems set to {}", !cur),
                                        Instant::now(),
                                    ));
                                }
                                "Remap controls" => {
                                    let actions = vec![
                                        "A".to_string(),
                                        "B".to_string(),
                                        "UP".to_string(),
                                        "DOWN".to_string(),
                                        "LEFT".to_string(),
                                        "RIGHT".to_string(),
                                        "START".to_string(),
                                    ];
                                    menu_next_state = Some(MenuState::Remap {
                                        actions,
                                        idx: 0,
                                        temp_map: HashMap::new(),
                                    });
                                    break;
                                }
                                "Reload config" => {
                                    let prev_system = systems_vec.get(*current_system_idx).cloned();
                                    *config = load_config();
                                    *groups =
                                        crate::scan::scan_grouped(Path::new(roms_dir), &*config);
                                    systems_vec.clear();
                                    if let Some(systems) = config.systems.as_ref() {
                                        for k in systems.keys() {
                                            let k_l = k.to_lowercase();
                                            let has_entries = groups
                                                .get(&k_l)
                                                .map(|v| !v.is_empty())
                                                .unwrap_or(false);
                                            if has_entries
                                                || config.show_empty_systems.unwrap_or(false)
                                            {
                                                systems_vec.push(k_l);
                                            }
                                        }
                                    }
                                    if let Some(prev) = prev_system {
                                        if let Some(pos) =
                                            systems_vec.iter().position(|s| s == &prev)
                                        {
                                            *current_system_idx = pos;
                                        } else {
                                            *current_system_idx = 0;
                                        }
                                    } else {
                                        *current_system_idx = 0;
                                    }
                                    let cur = systems_vec.get(*current_system_idx).cloned();
                                    *current_roms = cur
                                        .as_ref()
                                        .and_then(|s| groups.get(s).cloned())
                                        .unwrap_or_default();
                                    text_textures.clear();
                                    for _ in 0..current_roms.len() {
                                        text_textures.push(None);
                                    }
                                    menu_message =
                                        Some(("Config reloaded".to_string(), Instant::now()));
                                }
                                "Save config" => {
                                    if let Err(e) = write_config(config) {
                                        menu_message =
                                            Some((format!("Save failed: {}", e), Instant::now()));
                                    } else {
                                        menu_message =
                                            Some(("Config saved".to_string(), Instant::now()));
                                    }
                                }
                                "Close" => {
                                    menu_next_state = Some(MenuState::Closed);
                                }
                                "Exit" => {
                                    should_quit = true;
                                    menu_next_state = Some(MenuState::Closed);
                                }
                                _ => {}
                            }
                        }
                        sdl2::keyboard::Keycode::Escape => {
                            menu_next_state = Some(MenuState::Closed);
                        }
                        _ => {}
                    },
                    Event::ControllerButtonDown { button, .. } => match button {
                        sdl2::controller::Button::DPadUp => {
                            if *msel > 0 {
                                *msel -= 1;
                            }
                        }
                        sdl2::controller::Button::DPadDown => {
                            if *msel + 1 < items.len() {
                                *msel += 1
                            }
                        }
                        sdl2::controller::Button::A => {
                            let sel_label = items[*msel].as_str();
                            match sel_label {
                                "Toggle show_empty_systems" => {
                                    let cur = config.show_empty_systems.unwrap_or(false);
                                    config.show_empty_systems = Some(!cur);
                                    menu_message = Some((
                                        format!("show_empty_systems set to {}", !cur),
                                        Instant::now(),
                                    ));
                                }
                                "Remap controls" => {
                                    let actions = vec![
                                        "A".to_string(),
                                        "B".to_string(),
                                        "UP".to_string(),
                                        "DOWN".to_string(),
                                        "LEFT".to_string(),
                                        "RIGHT".to_string(),
                                        "START".to_string(),
                                    ];
                                    menu_next_state = Some(MenuState::Remap {
                                        actions,
                                        idx: 0,
                                        temp_map: HashMap::new(),
                                    });
                                    break;
                                }
                                "Save config" => {
                                    if let Err(e) = write_config(config) {
                                        menu_message =
                                            Some((format!("Save failed: {}", e), Instant::now()));
                                    } else {
                                        menu_message =
                                            Some(("Config saved".to_string(), Instant::now()));
                                    }
                                }
                                "Close" => {
                                    menu_next_state = Some(MenuState::Closed);
                                }
                                "Exit" => {
                                    should_quit = true;
                                    menu_next_state = Some(MenuState::Closed);
                                }
                                _ => {}
                            }
                        }
                        sdl2::controller::Button::B => {
                            menu_next_state = Some(MenuState::Closed);
                        }
                        _ => {}
                    },
                    Event::JoyButtonDown { button_idx, .. } => {
                        if *msel < items.len() {
                            if button_idx == 0 {
                                let sel_label = items[*msel].as_str();
                                if sel_label == "Remap controls" {
                                    let actions = vec![
                                        "A".to_string(),
                                        "B".to_string(),
                                        "UP".to_string(),
                                        "DOWN".to_string(),
                                        "LEFT".to_string(),
                                        "RIGHT".to_string(),
                                        "START".to_string(),
                                    ];
                                    menu_next_state = Some(MenuState::Remap {
                                        actions,
                                        idx: 0,
                                        temp_map: HashMap::new(),
                                    });
                                    break;
                                } else if sel_label == "Exit" {
                                    should_quit = true;
                                    menu_next_state = Some(MenuState::Closed);
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        MenuState::Remap {
            actions,
            idx,
            temp_map,
        } => {
            // draw remap overlay
            let overlay_rgba = Color::RGBA(0, 0, 0, 200);
            canvas.set_draw_color(overlay_rgba);
            let _ = canvas.fill_rect(Rect::new(
                0,
                0,
                canvas.output_size().unwrap_or((800, 600)).0,
                canvas.output_size().unwrap_or((800, 600)).1,
            ));
            let prompt = format!(
                "Press a button for: {}",
                actions.get(*idx).unwrap_or(&"".to_string())
            );
            if let Ok(surf) = font.render(&prompt).blended(Color::RGB(240, 240, 240)) {
                if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                    let q = tex.query();
                    let (w, h) = canvas.output_size().unwrap_or((800, 600));
                    let dst_x = ((w as i32) - q.width as i32) / 2;
                    let dst_y = (h as i32) / 2;
                    let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                }
            }
            canvas.present();

            if let Some(evt) = event_pump.wait_event_timeout(3000) {
                match evt {
                    Event::ControllerButtonDown { button, .. } => {
                        let key = format!("controller:{:?}", button);
                        if let Some(act) = actions.get(*idx).cloned() {
                            temp_map.insert(act, key);
                            *idx += 1;
                        }
                    }
                    Event::JoyButtonDown { button_idx, .. } => {
                        let key = format!("joybutton:{}", button_idx);
                        if let Some(act) = actions.get(*idx).cloned() {
                            temp_map.insert(act, key);
                            *idx += 1;
                        }
                    }
                    _ => {}
                }
            }

            if *idx >= actions.len() {
                config.controller_map = Some(temp_map.clone());
                if let Err(e) = write_config(config) {
                    menu_message = Some((format!("Save failed: {}", e), Instant::now()));
                } else {
                    menu_message = Some(("Controller mapping saved".to_string(), Instant::now()));
                }
                menu_next_state = Some(MenuState::Closed);
            }
        }
    }

    (menu_next_state, menu_message, should_quit)
}
