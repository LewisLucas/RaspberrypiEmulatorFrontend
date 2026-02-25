mod config;
mod emu;
mod scan;
mod style;
mod ui;

use sdl2::controller::Button as CButton;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::Texture;
use sdl2::ttf::Sdl2TtfContext;
use sdl2::video::FullscreenType;
use std::collections::HashMap;
use std::env;
#[cfg(feature = "x11")]
use std::ffi::CString;
use std::path::{Path, PathBuf};
#[cfg(feature = "x11")]
use std::ptr;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
#[cfg(feature = "x11")]
use x11::xlib;

use crate::config::{load_config, user_config_path, write_config};
use crate::emu::spawn_emulator_template;
use crate::scan::{find_system_for_extension, scan_grouped};
use crate::style::load_style;

const TILE_H: i32 = 140;

fn main() -> Result<(), String> {
    let roms_arg = env::args().nth(1);

    // load config (writes default sample if needed)
    let mut config = load_config();

    // determine roms dir: prefer CLI arg, else config.default_roms_path, else ./roms
    let roms_dir = match roms_arg {
        Some(d) => d,
        None => config
            .default_roms_path
            .clone()
            .unwrap_or_else(|| "./roms".to_string()),
    };

    // scan and group roms by top-level system folder
    let mut groups = scan_grouped(Path::new(&roms_dir), &config);

    // prepare systems list from config order (preserve config order if possible)
    let mut systems_vec: Vec<String> = Vec::new();
    if let Some(systems) = config.systems.as_ref() {
        for k in systems.keys() {
            let k_l = k.to_lowercase();
            // include system if it has entries or if user wants to show empty systems
            let has_entries = groups.get(&k_l).map(|v| !v.is_empty()).unwrap_or(false);
            if has_entries || config.show_empty_systems.unwrap_or(false) {
                systems_vec.push(k_l);
            }
        }
    }

    if systems_vec.is_empty() {
        eprintln!(
            "No configured systems found in config or no systems contain ROMs. Check {}",
            user_config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config/rpi_emulator_frontend/config.toml".to_string())
        );
    }

    // current system index
    let mut current_system_idx: usize = 0;
    // get current system name
    let current_system = systems_vec.get(current_system_idx).cloned();
    // current roms list for system
    let mut current_roms: Vec<PathBuf> = current_system
        .as_ref()
        .and_then(|s| groups.get(s).cloned())
        .unwrap_or_default();

    let sdl_ctx = sdl2::init()?;
    let video = sdl_ctx.video()?;
    let controller_subsystem = sdl_ctx.game_controller()?;

    let display_mode = video.desktop_display_mode(0)?;
    let (w, h) = (display_mode.w, display_mode.h);

    let window = video
        .window("RPI Frontend", w as u32, h as u32)
        .position_centered()
        .fullscreen() // fullscreen window
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;

    // initialize TTF
    let ttf_ctx: Sdl2TtfContext = sdl2::ttf::init().map_err(|e| e.to_string())?;

    // try to find a reasonable system font, allow override via FONT_PATH
    // font path preference order: config.font_path -> FONT_PATH env -> common system fonts
    let font_path = config
        .font_path
        .clone()
        .or_else(|| std::env::var("FONT_PATH").ok())
        .or_else(|| {
            let candidates = [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ];
            candidates
                .iter()
                .find(|p| Path::new(p).exists())
                .map(|s| s.to_string())
        });

    let font_path = match font_path {
        Some(p) => p,
        None => return Err("No TTF font found. Set font_path in config or install DejaVu/FreeSans or set FONT_PATH.".into()),
    };

    let font = ttf_ctx
        .load_font(font_path, 14)
        .map_err(|e| e.to_string())?;

    // load style/theme (writes a default style.toml in user config dir if missing)
    let style = load_style();
    let to_rgb = |arr: [u8; 3]| -> Color { Color::RGB(arr[0], arr[1], arr[2]) };
    let to_rgba = |arr: [u8; 3], a: u8| -> Color { Color::RGBA(arr[0], arr[1], arr[2], a) };
    let bg_color = to_rgb(style.background.unwrap_or([12, 12, 12]));
    let tile_selected_c = to_rgb(style.tile_selected.unwrap_or([200, 180, 50]));
    let tile_normal_c = to_rgb(style.tile_normal.unwrap_or([60, 60, 60]));
    let text_primary_c = to_rgb(style.text_primary.unwrap_or([240, 240, 240]));
    let text_secondary_c = to_rgb(style.text_secondary.unwrap_or([180, 180, 180]));
    let banner_bg_c = to_rgb(style.banner_bg.unwrap_or([20, 20, 20]));
    let banner_text_c = to_rgb(style.banner_text.unwrap_or([220, 220, 220]));
    let emu_text_c = to_rgb(style.emu_text.unwrap_or([180, 180, 180]));
    let overlay_base = style.overlay_bg.unwrap_or([0, 0, 0]);
    let overlay_alpha = style.overlay_alpha.unwrap_or(200);
    let overlay_rgba = to_rgba(overlay_base, overlay_alpha);
    let menu_bg_c = to_rgb(style.menu_bg.unwrap_or([10, 10, 10]));
    let menu_box_c = to_rgb(style.menu_box.unwrap_or([40, 40, 40]));
    let menu_selected_c = to_rgb(style.menu_selected.unwrap_or([80, 80, 80]));
    let menu_title_c = to_rgb(style.menu_title.unwrap_or([230, 230, 230]));
    let menu_text_c = to_rgb(style.menu_text.unwrap_or([220, 220, 220]));
    let message_overlay_rgba = to_rgba(
        style.overlay_bg.unwrap_or([0, 0, 0]),
        style.message_overlay_alpha.unwrap_or(160),
    );

    // Open controllers
    // Keep opened controllers alive by storing them in a vector; otherwise they get dropped
    let mut controllers: Vec<sdl2::controller::GameController> = Vec::new();
    for id in 0..sdl_ctx.joystick()?.num_joysticks()? {
        if controller_subsystem.is_game_controller(id) {
            match controller_subsystem.open(id) {
                Ok(gc) => {
                    println!("Opened controller: {}", gc.name());
                    controllers.push(gc);
                }
                Err(e) => eprintln!("Failed opening controller {}: {}", id, e),
            }
        }
    }

    // channel to receive global kill requests (from X11 hotkey thread)
    #[allow(unused_variables)]
    let (kill_tx, kill_rx) = mpsc::channel::<()>();

    // Spawn an X11 listener thread to capture a global hotkey (Ctrl+Alt+K) to kill the running emulator.
    // This is optional: enabled with the `x11` feature. If the feature is not enabled the listener
    // is skipped so the binary won't require X11 development libraries at link time.
    #[cfg(feature = "x11")]
    {
        let kill_tx = kill_tx.clone();
        thread::spawn(move || {
            unsafe {
                let display = xlib::XOpenDisplay(ptr::null());
                if display.is_null() {
                    eprintln!("XOpenDisplay failed, global hotkey not available");
                    return;
                }
                let root = xlib::XDefaultRootWindow(display);
                // keysym for 'K'
                let kstr = CString::new("K").unwrap();
                let keysym = xlib::XStringToKeysym(kstr.as_ptr());
                if keysym == 0 {
                    eprintln!("XStringToKeysym failed");
                    return;
                }
                let keycode = xlib::XKeysymToKeycode(display, keysym as u64);
                // grab Ctrl+Alt+K
                let modifiers = xlib::ControlMask | xlib::Mod1Mask;
                xlib::XGrabKey(
                    display,
                    keycode as i32,
                    modifiers as u32,
                    root,
                    1,
                    xlib::GrabModeAsync,
                    xlib::GrabModeAsync,
                );
                xlib::XSelectInput(display, root, xlib::KeyPressMask);
                loop {
                    let mut ev: xlib::XEvent = std::mem::zeroed();
                    xlib::XNextEvent(display, &mut ev);
                    if ev.type_ == xlib::KeyPress {
                        // send kill signal
                        let _ = kill_tx.send(());
                    }
                }
            }
        });
    }

    let (tx, rx) = mpsc::channel::<()>();

    // shared slot for the running child process so we can kill it from another thread
    let current_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));

    let mut error_overlay: Option<(String, Instant)> = None;

    // cache textures for filenames to avoid recreating each frame
    let texture_creator = canvas.texture_creator();
    // cache textures per-rom as multiple line textures (for current system)
    let mut text_textures: Vec<Option<Vec<Texture>>> = Vec::with_capacity(current_roms.len());
    for _ in 0..current_roms.len() {
        text_textures.push(None);
    }

    let mut event_pump = sdl_ctx.event_pump()?;
    // controller combo tracking: record button press times
    use std::collections::HashMap as Map;
    let mut pressed_buttons: Map<CButton, Instant> = Map::new();
    let combo_window = std::time::Duration::from_millis(400);
    let combo_cooldown = std::time::Duration::from_secs(1);
    let mut last_combo = Instant::now() - combo_cooldown;
    let mut selected: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut launching = false;
    let mut is_fullscreen = true;
    // menu state (moved to ui module)
    use crate::ui::MenuState;
    let mut menu_state = MenuState::Closed;
    let mut menu_message: Option<(String, Instant)> = None;
    let mut should_quit = false;

    'running: loop {
        // handle spawn completion
        if let Ok(_) = rx.try_recv() {
            launching = false;
        }

        // handle global kill requests (from X11 hotkey)
        if let Ok(_) = kill_rx.try_recv() {
            let mut slot = current_child.lock().unwrap();
            if let Some(ref mut c) = slot.as_mut() {
                match c.kill() {
                    Ok(_) => {
                        menu_message = Some(("Killed emulator".to_string(), Instant::now()));
                    }
                    Err(e) => {
                        menu_message = Some((format!("Kill failed: {}", e), Instant::now()));
                    }
                }
            } else {
                menu_message = Some(("No emulator running".to_string(), Instant::now()));
            }
        }

        // collect menu events when menu is open so main UI won't also react
        let mut menu_events: Vec<sdl2::event::Event> = Vec::new();

        for event in event_pump.poll_iter() {
            // If a menu or remap overlay is open, buffer events for the menu and skip main UI handling
            if let MenuState::Open { .. } | MenuState::Remap { .. } = menu_state {
                menu_events.push(event);
                continue;
            }
            match event {
                Event::Quit { .. } => break 'running,
                // allow opening the menu with 'C' regardless of launching state
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    ..
                } => {
                    let items = vec![
                        "Toggle show_empty_systems".to_string(),
                        "Remap controls".to_string(),
                        "Reload config".to_string(),
                        "Save config".to_string(),
                        "Close".to_string(),
                        "Exit".to_string(),
                    ];
                    menu_state = MenuState::Open { items, selected: 0 };
                    // try to raise the SDL window so menu is visually on top
                    let _ = canvas.window_mut().raise();
                    println!("Menu opened (key C)");
                }
                // allow opening the menu with the controller Start button even when other guards exist
                Event::ControllerButtonDown { button, .. } => {
                    // record press time for combo detection
                    pressed_buttons.insert(button, Instant::now());
                    // if button released events are not received we clear entries via timeout elsewhere
                    // check kill combo (Start + LeftShoulder + RightShoulder) within window
                    if last_combo.elapsed() >= combo_cooldown {
                        if pressed_buttons.contains_key(&CButton::Start)
                            && pressed_buttons.contains_key(&CButton::LeftShoulder)
                            && pressed_buttons.contains_key(&CButton::RightShoulder)
                        {
                            // ensure presses happened within combo_window
                            let times: Vec<_> = [
                                CButton::Start,
                                CButton::LeftShoulder,
                                CButton::RightShoulder,
                            ]
                            .iter()
                            .filter_map(|b| pressed_buttons.get(b))
                            .cloned()
                            .collect();
                            if times.len() == 3 {
                                let min = times.iter().min().unwrap();
                                let max = times.iter().max().unwrap();
                                if max.duration_since(*min) <= combo_window {
                                    // trigger kill
                                    last_combo = Instant::now();
                                    match emu::kill_current_emulator(&current_child) {
                                        Ok(m) => {
                                            menu_message = Some((m, Instant::now()));
                                            launching = false;
                                        }
                                        Err(e) => {
                                            menu_message = Some((
                                                format!("Kill failed: {}", e),
                                                Instant::now(),
                                            ));
                                        }
                                    }
                                    pressed_buttons.clear();
                                }
                            }
                        }
                    }
                    // existing Start-open-menu behavior preserved below when matched
                    if button == CButton::Start {
                        let items = vec![
                            "Toggle show_empty_systems".to_string(),
                            "Remap controls".to_string(),
                            "Reload config".to_string(),
                            "Save config".to_string(),
                            "Close".to_string(),
                            "Exit".to_string(),
                        ];
                        menu_state = MenuState::Open { items, selected: 0 };
                        let _ = canvas.window_mut().raise();
                        println!("Menu opened (controller Start)");
                    }
                }
                // joystick button events: map Start (common idx 7) to open menu; otherwise handle as joystick buttons
                Event::JoyButtonDown { button_idx, .. } => {
                    println!("Joystick button event idx: {}", button_idx);
                    // typical mapping: Start often appears as button index 7 on some drivers
                    if button_idx == 7 {
                        let items = vec![
                            "Toggle show_empty_systems".to_string(),
                            "Remap controls".to_string(),
                            "Reload config".to_string(),
                            "Save config".to_string(),
                            "Close".to_string(),
                            "Exit".to_string(),
                        ];
                        menu_state = MenuState::Open { items, selected: 0 };
                        let _ = canvas.window_mut().raise();
                        println!("Menu opened (joy idx 7)");
                        continue;
                    }
                    // if not launching, handle joystick button actions (fallback)
                    if !launching {
                        match button_idx {
                            0 => {
                                // common: A
                                if let Some(rom_path) = current_roms.get(selected).cloned() {
                                    if !systems_vec.is_empty() {
                                        if let Some(s) =
                                            systems_vec.get(current_system_idx).cloned()
                                        {
                                            if let Some(systems) = config.systems.as_ref() {
                                                if let Some(t) = systems.get(&s) {
                                                    launching = true;
                                                    let tx = tx.clone();
                                                    let t = t.clone();
                                                    let child_slot = current_child.clone();
                                                    thread::spawn(move || {
                                                        spawn_emulator_template(
                                                            &t, &rom_path, child_slot,
                                                        );
                                                        let _ = tx.send(());
                                                    });
                                                } else {
                                                    error_overlay = Some((
                                                        format!(
                                                            "No emulator configured for system {}",
                                                            s
                                                        ),
                                                        Instant::now(),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            1 => { /* B button: back / cancel */ }
                            _ => {}
                        }
                    }
                }
                Event::ControllerButtonUp { button, .. } => {
                    // remove from pressed set
                    pressed_buttons.remove(&button);
                }
                // Escape: close menu if open, otherwise quit
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => match menu_state {
                    MenuState::Open { .. } => {
                        menu_state = MenuState::Closed;
                    }
                    _ => break 'running,
                },
                Event::KeyDown {
                    keycode: Some(k), ..
                } if !launching => {
                    match k {
                        Keycode::C => {
                            // open settings menu (changed to 'C')
                            let items = vec![
                                "Toggle show_empty_systems".to_string(),
                                "Remap controls".to_string(),
                                "Reload config".to_string(),
                                "Save config".to_string(),
                                "Close".to_string(),
                                "Exit".to_string(),
                            ];
                            menu_state = MenuState::Open { items, selected: 0 };
                            println!("Menu opened (key C alt)");
                        }
                        Keycode::Left => {
                            // switch to previous system
                            if current_system_idx > 0 {
                                current_system_idx -= 1;
                            } else {
                                current_system_idx = systems_vec.len().saturating_sub(1);
                            }
                            // update current roms and reset selection
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur
                                .as_ref()
                                .and_then(|s| groups.get(s).cloned())
                                .unwrap_or_default();
                            selected = 0;
                            scroll_offset = 0;
                            text_textures.clear();
                            for _ in 0..current_roms.len() {
                                text_textures.push(None);
                            }
                        }
                        Keycode::Right => {
                            // switch to next system
                            current_system_idx = (current_system_idx + 1) % systems_vec.len();
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur
                                .as_ref()
                                .and_then(|s| groups.get(s).cloned())
                                .unwrap_or_default();
                            selected = 0;
                            scroll_offset = 0;
                            text_textures.clear();
                            for _ in 0..current_roms.len() {
                                text_textures.push(None);
                            }
                        }
                        Keycode::Up => {
                            if selected > 0 {
                                selected -= 1;
                                if selected < scroll_offset {
                                    scroll_offset = selected;
                                }
                            }
                        }
                        Keycode::Down => {
                            if selected + 1 < current_roms.len() {
                                selected += 1;
                                let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize;
                                if selected >= scroll_offset + visible {
                                    scroll_offset = selected - visible + 1;
                                }
                            }
                        }
                        Keycode::W => {
                            // toggle fullscreen/windowed for debugging
                            if is_fullscreen {
                                let _ = canvas.window_mut().set_fullscreen(FullscreenType::Off);
                                is_fullscreen = false;
                                println!("Toggled windowed mode");
                            } else {
                                let _ = canvas.window_mut().set_fullscreen(FullscreenType::Desktop);
                                is_fullscreen = true;
                                println!("Toggled fullscreen mode");
                            }
                        }
                        Keycode::Return => {
                            if let Some(rom_path) = current_roms.get(selected).cloned() {
                                let sys = systems_vec.get(current_system_idx).cloned();
                                if let Some(s) = sys {
                                    if let Some(systems) = config.systems.as_ref() {
                                        if let Some(t) = systems.get(&s) {
                                            launching = true;
                                            let tx = tx.clone();
                                            let t = t.clone();
                                            let child_slot = current_child.clone();
                                            thread::spawn(move || {
                                                spawn_emulator_template(&t, &rom_path, child_slot);
                                                let _ = tx.send(());
                                            });
                                        } else {
                                            // fallback: try resolve by extension across systems
                                            if let Some(ext) =
                                                rom_path.extension().and_then(|s| s.to_str())
                                            {
                                                let ext_l = ext.to_lowercase();
                                                if let Some(found_sys) = find_system_for_extension(
                                                    &ext_l,
                                                    &config,
                                                    &systems_vec,
                                                ) {
                                                    if let Some(found_t) = config
                                                        .systems
                                                        .as_ref()
                                                        .and_then(|m| m.get(&found_sys))
                                                    {
                                                        launching = true;
                                                        let tx = tx.clone();
                                                        let t = found_t.clone();
                                                        let child_slot = current_child.clone();
                                                        thread::spawn(move || {
                                                            spawn_emulator_template(
                                                                &t, &rom_path, child_slot,
                                                            );
                                                            let _ = tx.send(());
                                                        });
                                                    } else {
                                                        error_overlay = Some((format!("No emulator configured for system {}", found_sys), Instant::now()));
                                                    }
                                                } else {
                                                    error_overlay = Some((
                                                        format!(
                                                            "No emulator configured for system {}",
                                                            s
                                                        ),
                                                        Instant::now(),
                                                    ));
                                                }
                                            } else {
                                                error_overlay = Some((
                                                    format!(
                                                        "No emulator configured for system {}",
                                                        s
                                                    ),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // (Escape to quit is handled above)
                Event::ControllerButtonDown { button, .. } if !launching => {
                    println!("Controller button event: {:?}", button);
                    match button {
                        CButton::Start => {
                            // open settings menu
                            let items = vec![
                                "Toggle show_empty_systems".to_string(),
                                "Remap controls".to_string(),
                                "Reload config".to_string(),
                                "Save config".to_string(),
                                "Close".to_string(),
                                "Exit".to_string(),
                            ];
                            menu_state = MenuState::Open { items, selected: 0 };
                            println!("Menu opened (controller Start alt)");
                        }
                        CButton::DPadLeft => {
                            if current_system_idx > 0 {
                                current_system_idx -= 1;
                            } else {
                                current_system_idx = systems_vec.len().saturating_sub(1);
                            }
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur
                                .as_ref()
                                .and_then(|s| groups.get(s).cloned())
                                .unwrap_or_default();
                            selected = 0;
                            scroll_offset = 0;
                            text_textures.clear();
                            for _ in 0..current_roms.len() {
                                text_textures.push(None);
                            }
                        }
                        CButton::DPadRight => {
                            current_system_idx = (current_system_idx + 1) % systems_vec.len();
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur
                                .as_ref()
                                .and_then(|s| groups.get(s).cloned())
                                .unwrap_or_default();
                            selected = 0;
                            scroll_offset = 0;
                            text_textures.clear();
                            for _ in 0..current_roms.len() {
                                text_textures.push(None);
                            }
                        }
                        CButton::DPadUp => {
                            if selected > 0 {
                                selected -= 1;
                                if selected < scroll_offset {
                                    scroll_offset = selected;
                                }
                            }
                        }
                        CButton::DPadDown => {
                            if selected + 1 < current_roms.len() {
                                selected += 1;
                                let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize;
                                if selected >= scroll_offset + visible {
                                    scroll_offset = selected - visible + 1;
                                }
                            }
                        }
                        CButton::A => {
                            if let Some(rom_path) = current_roms.get(selected).cloned() {
                                if let Some(s) = systems_vec.get(current_system_idx).cloned() {
                                    if let Some(systems) = config.systems.as_ref() {
                                        if let Some(t) = systems.get(&s) {
                                            launching = true;
                                            let tx = tx.clone();
                                            let t = t.clone();
                                            let child_slot = current_child.clone();
                                            thread::spawn(move || {
                                                spawn_emulator_template(&t, &rom_path, child_slot);
                                                let _ = tx.send(());
                                            });
                                        } else {
                                            if let Some(ext) =
                                                rom_path.extension().and_then(|s| s.to_str())
                                            {
                                                let ext_l = ext.to_lowercase();
                                                if let Some(found_sys) = find_system_for_extension(
                                                    &ext_l,
                                                    &config,
                                                    &systems_vec,
                                                ) {
                                                    if let Some(found_t) = config
                                                        .systems
                                                        .as_ref()
                                                        .and_then(|m| m.get(&found_sys))
                                                    {
                                                        launching = true;
                                                        let tx = tx.clone();
                                                        let t = found_t.clone();
                                                        let child_slot = current_child.clone();
                                                        thread::spawn(move || {
                                                            spawn_emulator_template(
                                                                &t, &rom_path, child_slot,
                                                            );
                                                            let _ = tx.send(());
                                                        });
                                                    } else {
                                                        error_overlay = Some((format!("No emulator configured for system {}", found_sys), Instant::now()));
                                                    }
                                                } else {
                                                    error_overlay = Some((
                                                        format!(
                                                            "No emulator configured for system {}",
                                                            s
                                                        ),
                                                        Instant::now(),
                                                    ));
                                                }
                                            } else {
                                                error_overlay = Some((
                                                    format!(
                                                        "No emulator configured for system {}",
                                                        s
                                                    ),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        CButton::B => {
                            // placeholder: could go back from detail view
                        }
                        _ => {}
                    }
                }

                Event::JoyAxisMotion {
                    axis_idx, value, ..
                } if !launching => {
                    // axis_idx: 0 = left X, 1 = left Y
                    const AXIS_THRESHOLD: i16 = 16000;
                    if axis_idx == 0 {
                        // left/right switch systems
                        if value < -AXIS_THRESHOLD {
                            if !systems_vec.is_empty() {
                                if current_system_idx > 0 {
                                    current_system_idx -= 1;
                                } else {
                                    current_system_idx = systems_vec.len().saturating_sub(1);
                                }
                                let cur = systems_vec.get(current_system_idx).cloned();
                                current_roms = cur
                                    .as_ref()
                                    .and_then(|s| groups.get(s).cloned())
                                    .unwrap_or_default();
                                selected = 0;
                                scroll_offset = 0;
                                text_textures.clear();
                                for _ in 0..current_roms.len() {
                                    text_textures.push(None);
                                }
                            }
                        } else if value > AXIS_THRESHOLD {
                            if !systems_vec.is_empty() {
                                current_system_idx = (current_system_idx + 1) % systems_vec.len();
                                let cur = systems_vec.get(current_system_idx).cloned();
                                current_roms = cur
                                    .as_ref()
                                    .and_then(|s| groups.get(s).cloned())
                                    .unwrap_or_default();
                                selected = 0;
                                scroll_offset = 0;
                                text_textures.clear();
                                for _ in 0..current_roms.len() {
                                    text_textures.push(None);
                                }
                            }
                        }
                    } else if axis_idx == 1 {
                        // up/down navigate list
                        if value < -AXIS_THRESHOLD {
                            if selected > 0 {
                                selected -= 1;
                                if selected < scroll_offset {
                                    scroll_offset = selected;
                                }
                            }
                        } else if value > AXIS_THRESHOLD {
                            if selected + 1 < current_roms.len() {
                                selected += 1;
                                let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize;
                                if selected >= scroll_offset + visible {
                                    scroll_offset = selected - visible + 1;
                                }
                            }
                        }
                    }
                }
                // Menu input handling (when menu is open)
                // Note: we keep it simple and handle key/controller events in the main loop below when rendering the menu
                _ => {}
            }
        }

        // render main frame (list, banner, overlays)
        {
            use crate::ui::{render_frame, UIColors};
            let colors = UIColors {
                bg: bg_color,
                tile_selected: tile_selected_c,
                tile_normal: tile_normal_c,
                text_primary: text_primary_c,
                banner_bg: banner_bg_c,
                banner_text: banner_text_c,
                emu_text: emu_text_c,
                overlay_rgba,
            };
            render_frame(
                &mut canvas,
                &texture_creator,
                &font,
                &colors,
                &current_roms,
                &mut text_textures,
                selected,
                scroll_offset,
                current_system_idx,
                &systems_vec,
                w as i32,
                h as i32,
                launching,
                &mut error_overlay,
            );
        }

        // present moved after menu rendering so overlays are composed before presenting

        // handle menu input and rendering after presenting main content
        // menu state handling (delegated to ui::process_menu)
        let (menu_next, menu_msg_opt, menu_quit) = crate::ui::process_menu(
            &mut canvas,
            &texture_creator,
            &font,
            &mut menu_state,
            &mut menu_events,
            &mut config,
            &mut groups,
            &mut systems_vec,
            &mut current_system_idx,
            &roms_dir,
            &mut current_roms,
            &mut text_textures,
            &mut event_pump,
        );
        if let Some(m) = menu_msg_opt {
            menu_message = Some(m);
        }
        if let Some(s) = menu_next {
            menu_state = s;
        }
        if menu_quit {
            break 'running;
        }
        /* match &mut menu_state {
            MenuState::Closed => {}
            MenuState::Open {
                items,
                selected: msel,
            } => {
                //println!(
                //    "Rendering menu overlay, items={} selected={}",
                //    items.len(),
                //    msel
                //);
                // draw an opaque full-screen overlay so the menu is unmistakable
                canvas.set_draw_color(menu_bg_c);
                let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));

                // menu box
                let box_w = (w as i32) / 2;
                let box_h = (items.len() as i32) * 28 + 40;
                let box_x = (w as i32 - box_w) / 2;
                let box_y = (h as i32 - box_h) / 2;
                canvas.set_draw_color(menu_box_c);
                let _ = canvas.fill_rect(Rect::new(box_x, box_y, box_w as u32, box_h as u32));

                // Big MENU label
                if let Ok(surf_big) = font.render("MENU").blended(menu_text_c) {
                    if let Ok(tex_big) = texture_creator.create_texture_from_surface(&surf_big) {
                        let qb = tex_big.query();
                        let bx = box_x + 12;
                        let by = box_y + 8;
                        let _ = canvas.copy(&tex_big, None, Rect::new(bx, by, qb.width, qb.height));
                    }
                }

                // title
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

                // render items
                for (i, it) in items.iter().enumerate() {
                    let y = box_y + 40 + (i as i32) * 28;
                    if i == *msel {
                        canvas.set_draw_color(menu_selected_c);
                        let _ =
                            canvas.fill_rect(Rect::new(box_x + 8, y - 4, (box_w - 16) as u32, 28));
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
                            let _ = canvas.copy(
                                &tex,
                                None,
                                Rect::new(box_x + 16, y, q.width, q.height),
                            );
                        }
                    }
                }

                // menu overlay will be presented once per frame at the end of the render pass

                // process input for menu using the events collected earlier this frame
                for event in menu_events.drain(..) {
                    match event {
                        Event::KeyDown {
                            keycode: Some(k), ..
                        } => match k {
                            Keycode::Up => {
                                if *msel > 0 {
                                    *msel -= 1;
                                }
                            }
                            Keycode::Down => {
                                if *msel + 1 < items.len() {
                                    *msel += 1;
                                }
                            }
                            Keycode::Return => {
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
                                        // enter remap state
                                        let actions = vec![
                                            "A".to_string(),
                                            "B".to_string(),
                                            "UP".to_string(),
                                            "DOWN".to_string(),
                                            "LEFT".to_string(),
                                            "RIGHT".to_string(),
                                            "START".to_string(),
                                        ];
                                        let remap = MenuState::Remap {
                                            actions,
                                            idx: 0,
                                            temp_map: HashMap::new(),
                                        };
                                        menu_next_state = Some(remap);
                                        break;
                                    }
                                    "Reload config" => {
                                        // reload config from disk and re-scan roms
                                        let prev_system =
                                            systems_vec.get(current_system_idx).cloned();
                                        config = load_config();
                                        groups = scan_grouped(Path::new(&roms_dir), &config);

                                        // rebuild systems_vec
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

                                        // restore current_system_idx if possible
                                        if let Some(prev) = prev_system {
                                            if let Some(pos) =
                                                systems_vec.iter().position(|s| s == &prev)
                                            {
                                                current_system_idx = pos;
                                            } else {
                                                current_system_idx = 0;
                                            }
                                        } else {
                                            current_system_idx = 0;
                                        }

                                        // update current roms and textures
                                        let cur = systems_vec.get(current_system_idx).cloned();
                                        current_roms = cur
                                            .as_ref()
                                            .and_then(|s| groups.get(s).cloned())
                                            .unwrap_or_default();
                                        selected = 0;
                                        scroll_offset = 0;
                                        text_textures.clear();
                                        for _ in 0..current_roms.len() {
                                            text_textures.push(None);
                                        }

                                        menu_message =
                                            Some(("Config reloaded".to_string(), Instant::now()));
                                    }
                                    "Save config" => {
                                        if let Err(e) = write_config(&config) {
                                            menu_message = Some((
                                                format!("Save failed: {}", e),
                                                Instant::now(),
                                            ));
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
                            Keycode::Escape => {
                                menu_next_state = Some(MenuState::Closed);
                            }
                            _ => {}
                        },
                        Event::ControllerButtonDown { button, .. } => {
                            match button {
                                CButton::DPadUp => {
                                    if *msel > 0 {
                                        *msel -= 1;
                                    }
                                }
                                CButton::DPadDown => {
                                    if *msel + 1 < items.len() {
                                        *msel += 1;
                                    }
                                }
                                CButton::A => {
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
                                            let remap = MenuState::Remap {
                                                actions,
                                                idx: 0,
                                                temp_map: HashMap::new(),
                                            };
                                            menu_next_state = Some(remap);
                                            break;
                                        }
                                        "Reload config" => {
                                            menu_message = Some(("Reload not implemented in-menu; restart app to apply".to_string(), Instant::now()));
                                        }
                                        "Save config" => {
                                            if let Err(e) = write_config(&config) {
                                                menu_message = Some((
                                                    format!("Save failed: {}", e),
                                                    Instant::now(),
                                                ));
                                            } else {
                                                menu_message = Some((
                                                    "Config saved".to_string(),
                                                    Instant::now(),
                                                ));
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
                                CButton::B => {
                                    menu_next_state = Some(MenuState::Closed);
                                }
                                _ => {}
                            }
                        }
                        Event::JoyButtonDown { button_idx, .. } => {
                            // treat as pressing A when in menu to select
                            if *msel < items.len() {
                                // map button to selection
                                if button_idx == 0 {
                                    // common: A
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
                // apply any pending menu state change
                if let Some(s) = menu_next_state {
                    menu_state = s;
                }
                // If an Exit was chosen in the menu, break out of main loop
                if should_quit {
                    break 'running;
                }
            }
            MenuState::Remap {
                actions,
                idx,
                temp_map,
            } => {
                // draw remap overlay
                canvas.set_draw_color(overlay_rgba);
                let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
                let prompt = format!(
                    "Press a button for: {}",
                    actions.get(*idx).unwrap_or(&"".to_string())
                );
                if let Ok(surf) = font.render(&prompt).blended(text_primary_c) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                        let q = tex.query();
                        let dst_x = ((w as i32) - q.width as i32) / 2;
                        let dst_y = (h as i32) / 2;
                        let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                    }
                }
                canvas.present();

                // capture one event for remapping
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

                // finish
                if *idx >= actions.len() {
                    // commit to config
                    config.controller_map = Some(temp_map.clone());
                    if let Err(e) = write_config(&config) {
                        menu_message = Some((format!("Save failed: {}", e), Instant::now()));
                    } else {
                        menu_message =
                            Some(("Controller mapping saved".to_string(), Instant::now()));
                    }
                    menu_state = MenuState::Closed;
                }
            }
        }
        */

        // render menu message overlay if present (auto-hide after 3s)
        if let Some((ref msg, when)) = menu_message {
            if when.elapsed().as_secs() < 3 {
                canvas.set_draw_color(message_overlay_rgba);
                let _ = canvas.fill_rect(Rect::new(0, (h as i32) - 60, w as u32, 60));
                if let Ok(surf) = font.render(msg).blended(text_primary_c) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                        let q = tex.query();
                        let dst_x = 12;
                        let dst_y = h as i32 - 48;
                        let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                    }
                }
            } else {
                menu_message = None;
            }
        }
        // present final composition (main UI + possible menu overlay)
        canvas.present();

        // small delay
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    Ok(())
}
