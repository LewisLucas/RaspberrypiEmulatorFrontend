use crate::config::ConfigFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn scan_grouped(root: &Path, cfg: &ConfigFile) -> HashMap<String, Vec<PathBuf>> {
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
                        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                            if ignored_exts.contains(&ext.to_lowercase().as_str()) {
                                continue;
                            }
                        }
                        if let Ok(rel) = p.strip_prefix(root) {
                            let mut iter = rel.iter();
                            if let Some(first) = iter.next() {
                                if let Some(sys) = first.to_str() {
                                    let sys_l = sys.to_lowercase();
                                    if let Some(systems) = cfg.systems.as_ref() {
                                        if let Some(tmpl) = systems.get(&sys_l) {
                                            if let Some(visible) = tmpl.visible_extensions.as_ref()
                                            {
                                                if let Some(ext) =
                                                    p.extension().and_then(|s| s.to_str())
                                                {
                                                    if visible.iter().any(|e| {
                                                        e.to_lowercase() == ext.to_lowercase()
                                                    }) {
                                                        groups
                                                            .entry(sys_l)
                                                            .or_default()
                                                            .push(p.clone());
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

    for v in groups.values_mut() {
        v.sort();
    }
    groups
}

pub fn find_system_for_extension(
    ext: &str,
    cfg: &ConfigFile,
    systems_order: &Vec<String>,
) -> Option<String> {
    let ext_l = ext.to_lowercase();
    if let Some(systems) = cfg.systems.as_ref() {
        for sys in systems_order.iter() {
            if let Some(tmpl) = systems.get(sys) {
                if let Some(exts) = tmpl.extensions.as_ref() {
                    for e in exts.iter() {
                        if e.to_lowercase() == ext_l {
                            return Some(sys.clone());
                        }
                    }
                }
            }
        }
    }
    None
}
