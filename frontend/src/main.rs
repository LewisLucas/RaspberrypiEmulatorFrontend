use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::controller::Button as CButton;
use sdl2::rect::Rect;
use sdl2::pixels::Color;
use sdl2::ttf::Sdl2TtfContext;
use sdl2::render::Texture;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::process::Command;
use std::sync::{Arc, Mutex};
#[cfg(feature = "x11")]
use std::ffi::CString;
#[cfg(feature = "x11")]
use std::ptr;
#[cfg(feature = "x11")]
use x11::xlib;
use sdl2::video::FullscreenType;

const TILE_H: i32 = 140;

fn scan_grouped(root: &Path, cfg: &ConfigFile) -> HashMap<String, Vec<PathBuf>> {
    // group files by the top-level folder under root: roms/<system>/...
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let ignored_exts = ["zip", "7z", "rar", "gz", "xz"];

    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(cur) = stack.pop() {
        if let Ok(entries) = cur.read_dir() {
            for e in entries.flatten() {
                let p = e.path();
                match e.file_type() {
                    Ok(ft) if ft.is_dir() => stack.push(p),
                    Ok(ft) if ft.is_file() => {
                        // ignore archive files
                        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                            if ignored_exts.contains(&ext.to_lowercase().as_str()) { continue; }
                        }
                        if let Ok(rel) = p.strip_prefix(root) {
                            let mut iter = rel.iter();
                            if let Some(first) = iter.next() {
                                if let Some(sys) = first.to_str() {
                                    let sys_l = sys.to_lowercase();
                                    // only include if systems are configured and contain this key
                                    if let Some(systems) = cfg.systems.as_ref() {
                                        if let Some(tmpl) = systems.get(&sys_l) {
                                            // if visible_extensions is set, only include matching extensions
                                            if let Some(visible) = tmpl.visible_extensions.as_ref() {
                                                if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                                                    if visible.iter().any(|e| e.to_lowercase() == ext.to_lowercase()) {
                                                        groups.entry(sys_l).or_default().push(p.clone());
                                                    }
                                                }
                                            } else {
                                                groups.entry(sys_l).or_default().push(p.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // sort file lists for each system
    for v in groups.values_mut() { v.sort(); }
    groups
}

fn find_system_for_extension(ext: &str, cfg: &ConfigFile, systems_order: &Vec<String>) -> Option<String> {
    let ext_l = ext.to_lowercase();
    if let Some(systems) = cfg.systems.as_ref() {
        for sys in systems_order.iter() {
            if let Some(tmpl) = systems.get(sys) {
                if let Some(exts) = tmpl.extensions.as_ref() {
                    for e in exts.iter() {
                        if e.to_lowercase() == ext_l { return Some(sys.clone()); }
                    }
                }
            }
        }
    }
    None
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CmdTemplate {
    program: String,
    args: Vec<String>,
    extensions: Option<Vec<String>>,
    visible_extensions: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ConfigFile {
    default: Option<CmdTemplate>,
    systems: Option<HashMap<String, CmdTemplate>>,
    show_empty_systems: Option<bool>,
    controller_map: Option<HashMap<String, String>>,
    default_roms_path: Option<String>,
    font_path: Option<String>,
}

fn user_config_path() -> Option<std::path::PathBuf> {
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
    // Prefer a project-level config_template.toml in the current working directory if present.
    // This allows developers to provide a template at the repo root which will be copied to the
    // user's config location on first run. If not present, fall back to the built-in sample.
    let sample = if let Ok(template) = std::fs::read_to_string("config_template.toml") {
        template
    } else {
        include_str!("../config.sample.toml").to_string()
    };

    // atomic write
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, sample.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn load_config() -> ConfigFile {
    // default in-memory config if file missing
    let mut cfg = ConfigFile { default: Some(CmdTemplate { program: "mgba-qt".to_string(), args: vec!["{rom}".to_string()], extensions: None, visible_extensions: None }), systems: None, show_empty_systems: Some(false), controller_map: None, default_roms_path: None, font_path: None };
    if let Some(p) = user_config_path() {
        if !p.exists() {
            // write default sample for user to edit
            if let Err(e) = write_default_config(&p) {
                eprintln!("Failed to write default config: {}", e);
            }
        }
        if let Ok(contents) = std::fs::read_to_string(&p) {
            if let Ok(parsed) = toml::from_str::<ConfigFile>(&contents) {
                // merge into cfg
                if parsed.default.is_some() { cfg.default = parsed.default; }
                if parsed.systems.is_some() { cfg.systems = parsed.systems; }
                if parsed.show_empty_systems.is_some() { cfg.show_empty_systems = parsed.show_empty_systems; }
                if parsed.controller_map.is_some() { cfg.controller_map = parsed.controller_map; }
                if parsed.default_roms_path.is_some() { cfg.default_roms_path = parsed.default_roms_path; }
                if parsed.font_path.is_some() { cfg.font_path = parsed.font_path; }
            } else {
                eprintln!("Failed to parse config at {}", p.display());
            }
        }
    }
    cfg
}

fn write_config(cfg: &ConfigFile) -> Result<(), String> {
    if let Some(p) = user_config_path() {
        if let Some(parent) = p.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(format!("Failed to create config dir: {}", e));
            }
        }
        match toml::to_string_pretty(cfg) {
            Ok(s) => {
                let tmp = p.with_extension("toml.tmp");
                if let Err(e) = std::fs::write(&tmp, s.as_bytes()) { return Err(format!("Failed writing tmp config: {}", e)); }
                if let Err(e) = std::fs::rename(&tmp, &p) { return Err(format!("Failed renaming config: {}", e)); }
                return Ok(());
            }
            Err(e) => return Err(format!("Failed to serialize config: {}", e)),
        }
    }
    Err("No config path available".into())
}

// deprecated helper removed

fn spawn_emulator_template(tmpl: &CmdTemplate, rom: &Path, child_slot: Arc<Mutex<Option<std::process::Child>>>) {
    let mut cmd = Command::new(&tmpl.program);
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for a in &tmpl.args {
        if a == "{rom}" {
            args.push(rom.as_os_str().to_owned());
        } else {
            args.push(std::ffi::OsString::from(a));
        }
    }
    cmd.args(&args);
    match cmd.spawn() {
        Ok(child) => {
            println!("Launched {} with pid={}", tmpl.program, child.id());
            // place child into shared slot
            {
                let mut slot = child_slot.lock().unwrap();
                *slot = Some(child);
            }

            // wait using polling so other threads can lock and kill
            loop {
                // check child status
                {
                    let mut slot = child_slot.lock().unwrap();
                    if let Some(ref mut c) = slot.as_mut() {
                        match c.try_wait() {
                            Ok(Some(status)) => {
                                println!("Emulator exited with {:?}", status);
                                // remove from slot
                                slot.take();
                                break;
                            }
                            Ok(None) => {
                                // still running
                            }
                            Err(e) => {
                                eprintln!("Child try_wait error: {}", e);
                                slot.take();
                                break;
                            }
                        }
                    } else {
                        // no child present
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            println!("Emulator exited");
        }
        Err(e) => eprintln!("Failed to spawn emulator {}: {}", tmpl.program, e),
    }
}

fn main() -> Result<(), String> {
    let roms_arg = env::args().nth(1);

    // load config (writes default sample if needed)
    let mut config = load_config();

    // determine roms dir: prefer CLI arg, else config.default_roms_path, else ./roms
    let roms_dir = match roms_arg {
        Some(d) => d,
        None => config.default_roms_path.clone().unwrap_or_else(|| "./roms".to_string()),
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
        eprintln!("No configured systems found in config or no systems contain ROMs. Check {}", user_config_path().map(|p| p.display().to_string()).unwrap_or_else(|| "~/.config/rpi_emulator_frontend/config.toml".to_string()));
    }

    // current system index
    let mut current_system_idx: usize = 0;
    // get current system name
    let current_system = systems_vec.get(current_system_idx).cloned();
    // current roms list for system
    let mut current_roms: Vec<PathBuf> = current_system.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();

    let sdl_ctx = sdl2::init()?;
    let video = sdl_ctx.video()?;
    let controller_subsystem = sdl_ctx.game_controller()?;

    let display_mode = video.desktop_display_mode(0)?;
    let (w, h) = (display_mode.w, display_mode.h);

    let window = video.window("RPI Frontend", w as u32, h as u32)
        .position_centered()
        .fullscreen() // fullscreen window
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window.into_canvas().accelerated().present_vsync().build().map_err(|e| e.to_string())?;

    // initialize TTF
    let ttf_ctx: Sdl2TtfContext = sdl2::ttf::init().map_err(|e| e.to_string())?;

    // try to find a reasonable system font, allow override via FONT_PATH
    // font path preference order: config.font_path -> FONT_PATH env -> common system fonts
    let font_path = config.font_path.clone().or_else(|| std::env::var("FONT_PATH").ok())
        .or_else(|| {
            let candidates = [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ];
            candidates.iter().find(|p| Path::new(p).exists()).map(|s| s.to_string())
        });

    let font_path = match font_path {
        Some(p) => p,
        None => return Err("No TTF font found. Set font_path in config or install DejaVu/FreeSans or set FONT_PATH.".into()),
    };

    let font = ttf_ctx.load_font(font_path, 14).map_err(|e| e.to_string())?;

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
                xlib::XGrabKey(display, keycode as i32, modifiers as u32, root, 1, xlib::GrabModeAsync, xlib::GrabModeAsync);
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
    for _ in 0..current_roms.len() { text_textures.push(None); }

    let mut event_pump = sdl_ctx.event_pump()?;
    let mut selected: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut launching = false;
    let mut is_fullscreen = true;
    // menu state
    #[derive(PartialEq)]
    enum MenuState { Closed, Open { items: Vec<String>, selected: usize }, Remap { actions: Vec<String>, idx: usize, temp_map: HashMap<String,String> } }
    let mut menu_state = MenuState::Closed;
    let mut menu_message: Option<(String, Instant)> = None;

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
                Event::KeyDown { keycode: Some(Keycode::C), .. } => {
                    let items = vec!["Toggle show_empty_systems".to_string(), "Remap controls".to_string(), "Reload config".to_string(), "Save config".to_string(), "Close".to_string()];
                    menu_state = MenuState::Open { items, selected: 0 };
                    // try to raise the SDL window so menu is visually on top
                    let _ = canvas.window_mut().raise();
                    println!("Menu opened (key C)");
                }
                // allow opening the menu with the controller Start button even when other guards exist
                Event::ControllerButtonDown { button: CButton::Start, .. } => {
                    let items = vec!["Toggle show_empty_systems".to_string(), "Remap controls".to_string(), "Reload config".to_string(), "Save config".to_string(), "Close".to_string()];
                    menu_state = MenuState::Open { items, selected: 0 };
                    let _ = canvas.window_mut().raise();
                    println!("Menu opened (controller Start)");
                }
                // joystick button events: map Start (common idx 7) to open menu; otherwise handle as joystick buttons
                Event::JoyButtonDown { button_idx, .. } => {
                    println!("Joystick button event idx: {}", button_idx);
                    // typical mapping: Start often appears as button index 7 on some drivers
                        if button_idx == 7 {
                            let items = vec!["Toggle show_empty_systems".to_string(), "Remap controls".to_string(), "Reload config".to_string(), "Save config".to_string(), "Close".to_string()];
                            menu_state = MenuState::Open { items, selected: 0 };
                            let _ = canvas.window_mut().raise();
                            println!("Menu opened (joy idx 7)");
                            continue;
                        }
                    // if not launching, handle joystick button actions (fallback)
                    if !launching {
                        match button_idx {
                            0 => { // common: A
                                if let Some(rom_path) = current_roms.get(selected).cloned() {
                                    if !systems_vec.is_empty() {
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
                                                } else { error_overlay = Some((format!("No emulator configured for system {}", s), Instant::now())); }
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
                // Escape: close menu if open, otherwise quit
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    match menu_state {
                        MenuState::Open { .. } => { menu_state = MenuState::Closed; }
                        _ => break 'running,
                    }
                }
                        Event::KeyDown { keycode: Some(k), .. } if !launching => match k {
                            Keycode::C => {
                                // open settings menu (changed to 'C')
                                let items = vec!["Toggle show_empty_systems".to_string(), "Remap controls".to_string(), "Reload config".to_string(), "Save config".to_string(), "Close".to_string()];
                        menu_state = MenuState::Open { items, selected: 0 };
                        println!("Menu opened (key C alt)");
                            }
                    Keycode::Left => {
                        // switch to previous system
                        if current_system_idx > 0 {
                            current_system_idx -= 1;
                        } else { current_system_idx = systems_vec.len().saturating_sub(1); }
                        // update current roms and reset selection
                        let cur = systems_vec.get(current_system_idx).cloned();
                        current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();
                        selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); }
                    }
                    Keycode::Right => {
                        // switch to next system
                        current_system_idx = (current_system_idx + 1) % systems_vec.len();
                        let cur = systems_vec.get(current_system_idx).cloned();
                        current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();
                        selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); }
                    }
                    Keycode::Up => { if selected > 0 { selected -= 1; if selected < scroll_offset { scroll_offset = selected; } } }
                    Keycode::Down => { if selected + 1 < current_roms.len() { selected += 1; let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize; if selected >= scroll_offset + visible { scroll_offset = selected - visible + 1; } } }
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
                                        if let Some(ext) = rom_path.extension().and_then(|s| s.to_str()) {
                                            let ext_l = ext.to_lowercase();
                                            if let Some(found_sys) = find_system_for_extension(&ext_l, &config, &systems_vec) {
                                                if let Some(found_t) = config.systems.as_ref().and_then(|m| m.get(&found_sys)) {
                                                    launching = true;
                                                    let tx = tx.clone();
                                                    let t = found_t.clone();
                                                    let child_slot = current_child.clone();
                                                    thread::spawn(move || {
                                                        spawn_emulator_template(&t, &rom_path, child_slot);
                                                        let _ = tx.send(());
                                                    });
                                                } else {
                                                    error_overlay = Some((format!("No emulator configured for system {}", found_sys), Instant::now()));
                                                }
                                            } else {
                                                error_overlay = Some((format!("No emulator configured for system {}", s), Instant::now()));
                                            }
                                        } else {
                                            error_overlay = Some((format!("No emulator configured for system {}", s), Instant::now()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                },
                // (Escape to quit is handled above)
                Event::ControllerButtonDown { button, .. } if !launching => {
                    println!("Controller button event: {:?}", button);
                    match button {
                        CButton::Start => {
                            // open settings menu
                            let items = vec!["Toggle show_empty_systems".to_string(), "Remap controls".to_string(), "Reload config".to_string(), "Save config".to_string(), "Close".to_string()];
                            menu_state = MenuState::Open { items, selected: 0 };
                            println!("Menu opened (controller Start alt)");
                        }
                        CButton::DPadLeft => {
                            if current_system_idx > 0 { current_system_idx -= 1; } else { current_system_idx = systems_vec.len().saturating_sub(1); }
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();
                            selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); }
                        }
                        CButton::DPadRight => {
                            current_system_idx = (current_system_idx + 1) % systems_vec.len();
                            let cur = systems_vec.get(current_system_idx).cloned();
                            current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();
                            selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); }
                        }
                        CButton::DPadUp => { if selected > 0 { selected -= 1; if selected < scroll_offset { scroll_offset = selected; } } }
                        CButton::DPadDown => { if selected + 1 < current_roms.len() { selected += 1; let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize; if selected >= scroll_offset + visible { scroll_offset = selected - visible + 1; } } }
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
                                            if let Some(ext) = rom_path.extension().and_then(|s| s.to_str()) {
                                                let ext_l = ext.to_lowercase();
                                                if let Some(found_sys) = find_system_for_extension(&ext_l, &config, &systems_vec) {
                                                    if let Some(found_t) = config.systems.as_ref().and_then(|m| m.get(&found_sys)) {
                                                        launching = true;
                                                    let tx = tx.clone();
                                                    let t = found_t.clone();
                                                    let child_slot = current_child.clone();
                                                    thread::spawn(move || {
                                                        spawn_emulator_template(&t, &rom_path, child_slot);
                                                        let _ = tx.send(());
                                                    });
                                                    } else { error_overlay = Some((format!("No emulator configured for system {}", found_sys), Instant::now())); }
                                                } else { error_overlay = Some((format!("No emulator configured for system {}", s), Instant::now())); }
                                            } else { error_overlay = Some((format!("No emulator configured for system {}", s), Instant::now())); }
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
                
                
                Event::JoyAxisMotion { axis_idx, value, .. } if !launching => {
                    // axis_idx: 0 = left X, 1 = left Y
                    const AXIS_THRESHOLD: i16 = 16000;
                    if axis_idx == 0 {
                        // left/right switch systems
                        if value < -AXIS_THRESHOLD { if !systems_vec.is_empty() { if current_system_idx > 0 { current_system_idx -= 1; } else { current_system_idx = systems_vec.len().saturating_sub(1); } let cur = systems_vec.get(current_system_idx).cloned(); current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default(); selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); } } }
                        else if value > AXIS_THRESHOLD { if !systems_vec.is_empty() { current_system_idx = (current_system_idx + 1) % systems_vec.len(); let cur = systems_vec.get(current_system_idx).cloned(); current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default(); selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); } } }
                    } else if axis_idx == 1 {
                        // up/down navigate list
                        if value < -AXIS_THRESHOLD { if selected > 0 { selected -= 1; if selected < scroll_offset { scroll_offset = selected; } } }
                        else if value > AXIS_THRESHOLD { if selected + 1 < current_roms.len() { selected += 1; let visible = ((h as i32 - 60) / (TILE_H + 10)) as usize; if selected >= scroll_offset + visible { scroll_offset = selected - visible + 1; } } }
                    }
                }
                // Menu input handling (when menu is open)
                // Note: we keep it simple and handle key/controller events in the main loop below when rendering the menu
                _ => {}
            }
        }

        // render
        canvas.set_draw_color(Color::RGB(12, 12, 12));
        canvas.clear();

        // list layout (single column). compute tile sizes and visible window
        let padding = 10;
        let start_x = padding;
        let start_y = padding + 44; // leave space for banner
        let tile_w = (w as i32) - (padding * 2);
        let tile_h = TILE_H;

        let available_h = (h as i32) - start_y - padding;
        let visible = (available_h / (tile_h + padding)).max(1) as usize;

        // ensure scroll offset valid
        if scroll_offset >= current_roms.len() && !current_roms.is_empty() { scroll_offset = current_roms.len() - 1; }

        for (idx, rom) in current_roms.iter().enumerate().skip(scroll_offset).take(visible) {
            let i = idx;
            let x = start_x;
            let y = start_y + ((i - scroll_offset) as i32) * (tile_h + padding);
            let rect = Rect::new(x, y, tile_w as u32, tile_h as u32);

            if i == selected {
                canvas.set_draw_color(Color::RGB(200, 180, 50));
            } else {
                canvas.set_draw_color(Color::RGB(60, 60, 60));
            }
            let _ = canvas.fill_rect(rect);

            // filename text rendering (lazy create texture)
            if text_textures.get(i).and_then(|t| t.as_ref()).is_none() {
                if let Some(name) = rom.file_name().and_then(|s| s.to_str()) {
                    // Render filename into up to 2 lines. If too long, truncate the second line with ellipsis.
                    let padding = 8; // px padding inside tile
                    // use current list tile width, not the old TILE_W constant
                    let max_w = (tile_w as u32).saturating_sub((padding * 2) as u32);

                    // Helper to measure width using the font
                    let width_of = |s: &str| -> u32 {
                        font.size_of(s).map(|(w, _)| w).unwrap_or(0)
                    };

                    // If fits in one line, use that
                    if width_of(name) <= max_w {
                        if let Ok(surface) = font.render(name).blended(Color::RGB(240, 240, 240)) {
                            if let Ok(tex) = texture_creator.create_texture_from_surface(&surface) {
                                if let Some(slot) = text_textures.get_mut(i) {
                                    *slot = Some(vec![tex]);
                                }
                            }
                        }
                    } else {
                        // find maximal prefix that fits on first line (binary search)
                        let chars: Vec<char> = name.chars().collect();
                        let mut lo = 0usize;
                        let mut hi = chars.len();
                        while lo < hi {
                            let mid = (lo + hi + 1) / 2;
                            let cand: String = chars.iter().take(mid).collect();
                            if width_of(&cand) <= max_w { lo = mid; } else { hi = mid -1; }
                        }
                        let mut first: String = chars.iter().take(lo).collect();
                        let remaining: String = chars.iter().skip(lo).collect();

                        // Try to smart-split at the last separator within the first line
                        let seps = [' ', '-', ':', '_'];
                        if let Some(pos) = first.rfind(|c: char| seps.contains(&c)) {
                            // split at separator pos (exclude separator)
                            let new_first: String = first.chars().take(pos).collect();
                            if !new_first.is_empty() {
                                // remaining becomes text after separator plus old remaining
                                let after_sep: String = first.chars().skip(pos + 1).collect::<String>() + &remaining;
                                first = new_first;
                                // use after_sep as the new remaining
                                let remaining = after_sep;
                                // proceed to render second line based on new remaining
                                // determine second line below using 'remaining'
                                // For scope reasons we shadow the name 'remaining' by reassigning below via let
                                let remaining = remaining;

                                // Now create second line from remaining (fits or truncated)
                                let second = if width_of(&remaining) <= max_w { remaining } else {
                                    // truncate with ellipsis at end
                                    let ell = "...";
                                    let mut lo2 = 0usize; let mut hi2 = remaining.chars().count();
                                    while lo2 < hi2 {
                                        let mid = (lo2 + hi2 + 1) / 2;
                                        let cand: String = remaining.chars().take(mid).collect::<String>() + ell;
                                        if width_of(&cand) <= max_w { lo2 = mid; } else { hi2 = mid -1; }
                                    }
                                    let kept: String = remaining.chars().take(lo2).collect();
                                    if kept.is_empty() { ell.to_string() } else { kept + ell }
                                };

                                // render both lines
                                let mut line_texts: Vec<Texture> = Vec::new();
                                if let Ok(s1) = font.render(&first).blended(Color::RGB(240, 240, 240)) {
                                    if let Ok(t1) = texture_creator.create_texture_from_surface(&s1) { line_texts.push(t1); }
                                }
                                if let Ok(s2) = font.render(&second).blended(Color::RGB(240, 240, 240)) {
                                    if let Ok(t2) = texture_creator.create_texture_from_surface(&s2) { line_texts.push(t2); }
                                }
                                if let Some(slot) = text_textures.get_mut(i) { *slot = Some(line_texts); }
                                continue;
                            }
                        }

                        // Fallback behavior: second line is remaining, possibly truncated with ellipsis
                        let second = if width_of(&remaining) <= max_w { remaining.clone() } else {
                            let ell = "...";
                            let mut lo2 = 0usize; let mut hi2 = remaining.chars().count();
                            while lo2 < hi2 {
                                let mid = (lo2 + hi2 + 1) / 2;
                                let cand: String = remaining.chars().take(mid).collect::<String>() + ell;
                                if width_of(&cand) <= max_w { lo2 = mid; } else { hi2 = mid -1; }
                            }
                            let kept: String = remaining.chars().take(lo2).collect();
                            if kept.is_empty() { ell.to_string() } else { kept + ell }
                        };

                        // render both lines
                        let mut line_texts: Vec<Texture> = Vec::new();
                        if let Ok(s1) = font.render(&first).blended(Color::RGB(240, 240, 240)) {
                            if let Ok(t1) = texture_creator.create_texture_from_surface(&s1) { line_texts.push(t1); }
                        }
                        if let Ok(s2) = font.render(&second).blended(Color::RGB(240, 240, 240)) {
                            if let Ok(t2) = texture_creator.create_texture_from_surface(&s2) { line_texts.push(t2); }
                        }
                        if let Some(slot) = text_textures.get_mut(i) { *slot = Some(line_texts); }
                    }
                }
            }

            if let Some(Some(text_vec)) = text_textures.get(i) {
                // draw one or two lines centered vertically in the tile
                let mut total_h = 0i32;
                let mut queries: Vec<sdl2::render::TextureQuery> = Vec::new();
                for tex in text_vec.iter() {
                    let q = tex.query();
                    total_h += q.height as i32;
                    queries.push(q);
                }
                // spacing between lines
                let spacing = 2;
                total_h += spacing * ((queries.len() as i32) - 1).max(0);
                let mut cursor_y = y + (tile_h - total_h) / 2; // center vertically
                for (idx, tex) in text_vec.iter().enumerate() {
                    let q = &queries[idx];
                    let tex_w = q.width as i32;
                    let tex_h = q.height as i32;
                    let dst_x = x + (tile_w - tex_w) / 2;
                    let dst_y = cursor_y;
                    let _ = canvas.copy(tex, None, Rect::new(dst_x, dst_y, tex_w as u32, tex_h as u32));
                    cursor_y += tex_h + spacing;
                }
            }
        }

        // banner
        canvas.set_draw_color(Color::RGB(20, 20, 20));
        let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, 40));

        // render banner text: current system and selected filename + mapped emulator
        let current_system_name = systems_vec.get(current_system_idx).cloned().unwrap_or_else(|| "".to_string());
        // show system name + count
        let count = current_roms.len();
        let system_label = format!("{} ({})", current_system_name.to_uppercase(), count);
        if let Ok(surf_sys) = font.render(&system_label).blended(Color::RGB(220,220,220)) {
            if let Ok(tex_sys) = texture_creator.create_texture_from_surface(&surf_sys) {
                let q = tex_sys.query();
                // position system label at the right side of banner to avoid overlapping centered filename
                let dst_x = (w as i32) - (q.width as i32) - 12;
                let dst_y = 8;
                let _ = canvas.copy(&tex_sys, None, Rect::new(dst_x, dst_y, q.width, q.height));
            }
        }

        if let Some(rom_path) = current_roms.get(selected) {
            if let Some(name) = rom_path.file_name().and_then(|s| s.to_str()) {
                // emulator mapping name
                let emu_name = config.systems.as_ref().and_then(|m| m.get(&current_system_name)).map(|t| t.program.clone()).or_else(|| config.default.as_ref().map(|d| d.program.clone()));

                // prepare filename display: if too wide, do middle elide keeping start and end
                let banner_padding = 12u32;
                let avail = (w as u32).saturating_sub(banner_padding * 2);
                let full_name = name.to_string();
                let display_name = if font.size_of(&full_name).map(|(w,_)| w).unwrap_or(0) <= avail {
                    full_name.clone()
                } else {
                    // middle elide
                    fn elide_middle(s: &str, max_chars: usize) -> String {
                        let chars: Vec<char> = s.chars().collect();
                        if chars.len() <= max_chars { return s.to_string(); }
                        if max_chars <= 3 { return "...".to_string(); }
                        let keep = (max_chars - 3) / 2;
                        let head = keep + ((max_chars - 3) % 2);
                        let tail = keep;
                        let start: String = chars.iter().take(head).collect();
                        let end: String = chars.iter().rev().take(tail).collect::<Vec<&char>>().into_iter().rev().collect();
                        format!("{}...{}", start, end)
                    }
                    // estimate max chars fitting in avail using avg char width of 7
                    let est = ((avail as f32) / 7.0) as usize;
                    elide_middle(&full_name, est.max(8))
                };

                if let Ok(surf) = font.render(&display_name).blended(Color::RGB(220,220,220)) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                        let q = tex.query();
                        let dst_x = ((w as i32) - q.width as i32) / 2;
                        let dst_y = 8;
                        let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                    }
                }

                if let Some(emu) = emu_name {
                    let emu_txt = format!("emu: {}", emu);
                    if let Ok(surf2) = font.render(&emu_txt).blended(Color::RGB(180,180,180)) {
                        if let Ok(tex2) = texture_creator.create_texture_from_surface(&surf2) {
                            let q2 = tex2.query();
                            let dst_x2 = 12;
                            let dst_y2 = 10;
                            let _ = canvas.copy(&tex2, None, Rect::new(dst_x2, dst_y2, q2.width, q2.height));
                        }
                    }
                }
            }
        }

        // launching overlay
        if launching {
            canvas.set_draw_color(Color::RGBA(0, 0, 0, 200));
            let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
        }

        // error overlay for missing mapping or spawn errors (auto-hide after 3s)
        if let Some((ref msg, when)) = error_overlay {
            if when.elapsed().as_secs() < 3 {
                canvas.set_draw_color(Color::RGBA(0, 0, 0, 200));
                let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
                // render message centered top
                if let Ok(surface) = font.render(msg).blended(Color::RGB(240,240,240)) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surface) {
                        let q = tex.query();
                        let dst_x = (w as i32 - q.width as i32) / 2;
                        let dst_y = (h as i32 - q.height as i32) / 2;
                        let _ = canvas.copy(&tex, None, Rect::new(dst_x, dst_y, q.width, q.height));
                    }
                }
            } else {
                error_overlay = None;
            }
        }

        // present moved after menu rendering so overlays are composed before presenting

        // handle menu input and rendering after presenting main content
        // menu state handling
        let mut menu_next_state: Option<MenuState> = None;
        match &mut menu_state {
            MenuState::Closed => {}
            MenuState::Open { items, selected: msel } => {
                println!("Rendering menu overlay, items={} selected={}", items.len(), msel);
                // draw an opaque full-screen overlay so the menu is unmistakable
                canvas.set_draw_color(Color::RGB(10, 10, 10));
                let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));

                // menu box
                let box_w = (w as i32) / 2;
                let box_h = (items.len() as i32) * 28 + 40;
                let box_x = (w as i32 - box_w) / 2;
                let box_y = (h as i32 - box_h) / 2;
                canvas.set_draw_color(Color::RGB(40, 40, 40));
                let _ = canvas.fill_rect(Rect::new(box_x, box_y, box_w as u32, box_h as u32));

                // Big MENU label
                if let Ok(surf_big) = font.render("MENU").blended(Color::RGB(220,220,220)) {
                    if let Ok(tex_big) = texture_creator.create_texture_from_surface(&surf_big) {
                        let qb = tex_big.query();
                        let bx = box_x + 12;
                        let by = box_y + 8;
                        let _ = canvas.copy(&tex_big, None, Rect::new(bx, by, qb.width, qb.height));
                    }
                }

                // title
                if let Ok(surf) = font.render("Settings").blended(Color::RGB(230,230,230)) {
                    if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                        let q = tex.query();
                        let _ = canvas.copy(&tex, None, Rect::new(box_x + 12, box_y + 8, q.width, q.height));
                    }
                }

                // render items
                for (i, it) in items.iter().enumerate() {
                    let y = box_y + 40 + (i as i32) * 28;
                    if i == *msel {
                        canvas.set_draw_color(Color::RGB(80, 80, 80));
                        let _ = canvas.fill_rect(Rect::new(box_x + 8, y - 4, (box_w - 16) as u32, 28));
                    }
                    let label = if it == "Toggle show_empty_systems" {
                        let val = config.show_empty_systems.unwrap_or(false);
                        format!("{}: {}", it, if val { "ON" } else { "OFF" })
                    } else { it.clone() };

                    if let Ok(surf) = font.render(&label).blended(Color::RGB(220,220,220)) {
                        if let Ok(tex) = texture_creator.create_texture_from_surface(&surf) {
                            let q = tex.query();
                            let _ = canvas.copy(&tex, None, Rect::new(box_x + 16, y, q.width, q.height));
                        }
                    }
                }

                // menu overlay will be presented once per frame at the end of the render pass

                // process input for menu using the events collected earlier this frame
                for event in menu_events.drain(..) {
                    match event {
                        Event::KeyDown { keycode: Some(k), .. } => match k {
                            Keycode::Up => { if *msel > 0 { *msel -= 1; } }
                            Keycode::Down => { if *msel + 1 < items.len() { *msel += 1; } }
                            Keycode::Return => {
                                let sel_label = items[*msel].as_str();
                                match sel_label {
                                    "Toggle show_empty_systems" => {
                                        let cur = config.show_empty_systems.unwrap_or(false);
                                        config.show_empty_systems = Some(!cur);
                                        menu_message = Some((format!("show_empty_systems set to {}", !cur), Instant::now()));
                                    }
                                    "Remap controls" => {
                                        // enter remap state
                                        let actions = vec!["A".to_string(), "B".to_string(), "UP".to_string(), "DOWN".to_string(), "LEFT".to_string(), "RIGHT".to_string(), "START".to_string()];
                                        let remap = MenuState::Remap { actions, idx: 0, temp_map: HashMap::new() };
                                        menu_next_state = Some(remap);
                                        break;
                                    }
                                    "Reload config" => {
                                        // reload config from disk and re-scan roms
                                        let prev_system = systems_vec.get(current_system_idx).cloned();
                                        config = load_config();
                                        groups = scan_grouped(Path::new(&roms_dir), &config);

                                        // rebuild systems_vec
                                        systems_vec.clear();
                                        if let Some(systems) = config.systems.as_ref() {
                                            for k in systems.keys() {
                                                let k_l = k.to_lowercase();
                                                let has_entries = groups.get(&k_l).map(|v| !v.is_empty()).unwrap_or(false);
                                                if has_entries || config.show_empty_systems.unwrap_or(false) {
                                                    systems_vec.push(k_l);
                                                }
                                            }
                                        }

                                        // restore current_system_idx if possible
                                        if let Some(prev) = prev_system {
                                            if let Some(pos) = systems_vec.iter().position(|s| s == &prev) { current_system_idx = pos; }
                                            else { current_system_idx = 0; }
                                        } else { current_system_idx = 0; }

                                        // update current roms and textures
                                        let cur = systems_vec.get(current_system_idx).cloned();
                                        current_roms = cur.as_ref().and_then(|s| groups.get(s).cloned()).unwrap_or_default();
                                        selected = 0; scroll_offset = 0; text_textures.clear(); for _ in 0..current_roms.len() { text_textures.push(None); }

                                        menu_message = Some(("Config reloaded".to_string(), Instant::now()));
                                    }
                                    "Save config" => {
                                        if let Err(e) = write_config(&config) { menu_message = Some((format!("Save failed: {}", e), Instant::now())); }
                                        else { menu_message = Some(("Config saved".to_string(), Instant::now())); }
                                    }
                                    "Close" => { menu_next_state = Some(MenuState::Closed); }
                                    _ => {}
                                }
                            }
                    Keycode::Escape => { menu_next_state = Some(MenuState::Closed); }
                            _ => {}
                        },
                Event::ControllerButtonDown { button, .. } => match button {
                            CButton::DPadUp => { if *msel > 0 { *msel -= 1; } }
                            CButton::DPadDown => { if *msel + 1 < items.len() { *msel += 1; } }
                            CButton::A => {
                                let sel_label = items[*msel].as_str();
                                match sel_label {
                                    "Toggle show_empty_systems" => {
                                        let cur = config.show_empty_systems.unwrap_or(false);
                                        config.show_empty_systems = Some(!cur);
                                        menu_message = Some((format!("show_empty_systems set to {}", !cur), Instant::now()));
                                    }
                                    "Remap controls" => {
                                        let actions = vec!["A".to_string(), "B".to_string(), "UP".to_string(), "DOWN".to_string(), "LEFT".to_string(), "RIGHT".to_string(), "START".to_string()];
                                        let remap = MenuState::Remap { actions, idx: 0, temp_map: HashMap::new() };
                                        menu_next_state = Some(remap);
                                        break;
                                    }
                                    "Reload config" => { menu_message = Some(("Reload not implemented in-menu; restart app to apply".to_string(), Instant::now())); }
                                    "Save config" => { if let Err(e) = write_config(&config) { menu_message = Some((format!("Save failed: {}", e), Instant::now())); } else { menu_message = Some(("Config saved".to_string(), Instant::now())); } }
                                    "Close" => { menu_next_state = Some(MenuState::Closed); }
                                    _ => {}
                                }
                            }
                            CButton::B => { menu_next_state = Some(MenuState::Closed); }
                            _ => {}
                        },
                        Event::JoyButtonDown { button_idx, .. } => {
                            // treat as pressing A when in menu to select
                            if *msel < items.len() {
                                // map button to selection
                                if button_idx == 0 { // common: A
                                    let sel_label = items[*msel].as_str();
                                    if sel_label == "Remap controls" {
                                        let actions = vec!["A".to_string(), "B".to_string(), "UP".to_string(), "DOWN".to_string(), "LEFT".to_string(), "RIGHT".to_string(), "START".to_string()];
                                        menu_next_state = Some(MenuState::Remap { actions, idx: 0, temp_map: HashMap::new() });
                                        break;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // apply any pending menu state change
                if let Some(s) = menu_next_state { menu_state = s; }
            }
            MenuState::Remap { actions, idx, temp_map } => {
                // draw remap overlay
                canvas.set_draw_color(Color::RGBA(0, 0, 0, 200));
                let _ = canvas.fill_rect(Rect::new(0, 0, w as u32, h as u32));
                let prompt = format!("Press a button for: {}", actions.get(*idx).unwrap_or(&"".to_string()));
                if let Ok(surf) = font.render(&prompt).blended(Color::RGB(240,240,240)) {
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
                    if let Err(e) = write_config(&config) { menu_message = Some((format!("Save failed: {}", e), Instant::now())); }
                    else { menu_message = Some(("Controller mapping saved".to_string(), Instant::now())); }
                    menu_state = MenuState::Closed;
                }
            }
        }

        // render menu message overlay if present (auto-hide after 3s)
        if let Some((ref msg, when)) = menu_message {
            if when.elapsed().as_secs() < 3 {
                canvas.set_draw_color(Color::RGBA(0, 0, 0, 160));
                let _ = canvas.fill_rect(Rect::new(0, (h as i32) - 60, w as u32, 60));
                if let Ok(surf) = font.render(msg).blended(Color::RGB(240,240,240)) {
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
