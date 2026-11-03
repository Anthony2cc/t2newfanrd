use std::path::{Path, PathBuf};

/// Locate all Apple SMC fans through the hwmon interface.
///
/// macsmc-hwmon may expose both fans on the same hwmon node, or split them
/// across several nodes, so every Apple-related hwmon device is scanned and
/// each `fanX_input` with a writable `fanX_target` sibling is collected.
pub fn find_fan_paths() -> Vec<PathBuf> {
    let mut fans = Vec::new();

    let Ok(name_paths) = glob::glob("/sys/class/hwmon/hwmon*/name") else {
        return fans;
    };

    for name_path in name_paths.filter_map(Result::ok) {
        let Ok(name) = std::fs::read_to_string(&name_path) else {
            continue;
        };
        let name = name.trim();
        if !(name.starts_with("macsmc") || name.starts_with("apple")) {
            continue;
        }

        let Some(hwmon_dir) = name_path.parent() else {
            continue;
        };
        let fan_glob = hwmon_dir.join("fan*_input").display().to_string();

        let Ok(inputs) = glob::glob(&fan_glob) else {
            continue;
        };
        for path in inputs.filter_map(Result::ok) {
            let Some(fan_name) = path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .and_then(|file_name| file_name.strip_suffix("_input"))
                .map(str::to_owned)
            else {
                continue;
            };

            // Ignore read-only fan entries from drivers without a target knob.
            if !hwmon_dir.join(format!("{fan_name}_target")).exists() {
                continue;
            }

            let mut fan_path = path;
            fan_path.set_file_name(fan_name);
            fans.push(fan_path);
        }
    }

    fans.sort();
    fans.dedup();
    fans
}

/// `/sys/.../fan1` + `_min` -> `/sys/.../fan1_min`
pub fn file_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut result = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    result.set_file_name(format!("{file_name}{suffix}"));
    result
}

pub fn read_u32(path: &Path, suffix: &str) -> Option<u32> {
    std::fs::read_to_string(file_with_suffix(path, suffix))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn read_text(path: &Path, suffix: &str) -> Option<String> {
    Some(
        std::fs::read_to_string(file_with_suffix(path, suffix))
            .ok()?
            .trim()
            .to_owned(),
    )
}
