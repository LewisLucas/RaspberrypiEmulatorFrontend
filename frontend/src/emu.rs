use crate::config::CmdTemplate;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

pub fn spawn_emulator_template(
    tmpl: &CmdTemplate,
    rom: &Path,
    child_slot: Arc<Mutex<Option<std::process::Child>>>,
) {
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
            {
                let mut slot = child_slot.lock().unwrap();
                *slot = Some(child);
            }

            loop {
                {
                    let mut slot = child_slot.lock().unwrap();
                    if let Some(ref mut c) = slot.as_mut() {
                        match c.try_wait() {
                            Ok(Some(status)) => {
                                println!("Emulator exited with {:?}", status);
                                slot.take();
                                break;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                eprintln!("Child try_wait error: {}", e);
                                slot.take();
                                break;
                            }
                        }
                    } else {
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

/// Kill the currently running emulator if any. Returns a user-facing message on success or
/// an Err string on failure.
pub fn kill_current_emulator(
    child_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<String, String> {
    let mut slot = child_slot
        .lock()
        .map_err(|e| format!("failed to lock child slot: {}", e))?;
    if let Some(ref mut c) = slot.as_mut() {
        // try to kill; ignore ESRCH etc and report errors
        match c.kill() {
            Ok(_) => {
                // poll for exit for up to 1s
                let start = std::time::Instant::now();
                loop {
                    match c.try_wait() {
                        Ok(Some(status)) => {
                            // child exited
                            slot.take();
                            return Ok(format!("Emulator killed (status: {})", status));
                        }
                        Ok(None) => {
                            if start.elapsed() > std::time::Duration::from_secs(1) {
                                // give up, still running
                                // remove from slot to avoid dangling handle
                                slot.take();
                                return Ok("Emulator kill signalled".to_string());
                            }
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            continue;
                        }
                        Err(e) => {
                            // error while waiting; remove slot and return
                            slot.take();
                            return Err(format!("error waiting for process: {}", e));
                        }
                    }
                }
            }
            Err(e) => return Err(format!("failed to kill process: {}", e)),
        }
    }
    Err("No emulator running".to_string())
}
