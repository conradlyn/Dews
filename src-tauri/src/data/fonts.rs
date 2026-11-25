//! Enumerate fonts installed on the machine for the three font pickers.
//! Windows keeps the human-readable family names in the registry Fonts key;
//! HKLM covers system fonts, HKCU covers per-user installed fonts.
//! macOS walks the standard font directories and uses file stems as family names.

/// Fallback list used when the registry cannot be read (or on non-Windows).
pub fn fallback_fonts() -> Vec<String> {
    [
        "Microsoft YaHei",
        "Microsoft YaHei UI",
        "SimSun",
        "SimHei",
        "KaiTi",
        "FangSong",
        "DengXian",
        "PingFang SC",
        "Source Han Sans SC",
        "Noto Sans CJK SC",
        "Arial",
        "Segoe UI",
        "Times New Roman",
        "Calibri",
        "Consolas",
        "Georgia",
        "Verdana",
        "Tahoma",
        "Courier New",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(windows)]
fn tidy(raw: &str) -> Option<String> {
    let mut name = raw.trim();
    if name.is_empty() {
        return None;
    }
    // Registry values look like "Arial (TrueType)" / "Segoe UI (OpenType)".
    for suffix in [
        " (TrueType)",
        " (OpenType)",
        " (All res)",
        " (TrueType, All res)",
        " (OpenType, All res)",
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim();
            break;
        }
    }
    if name.is_empty() || name.len() > 120 {
        return None;
    }
    Some(name.to_string())
}

#[cfg(windows)]
fn from_key(root: isize, subkey: &str) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ,
    };
    let root = match root {
        0 => HKEY_LOCAL_MACHINE,
        1 => HKEY_CURRENT_USER,
        _ => return Vec::new(),
    };
    let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey: HKEY = 0;
    let rc = unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_READ, &mut hkey) };
    if rc != 0 {
        return Vec::new();
    }
    let mut count: u32 = 0;
    let mut max_len: u32 = 0;
    unsafe {
        RegQueryInfoKeyW(
            hkey,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut count,
            &mut max_len,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    let cap = (max_len as usize + 2).clamp(128, 2048);
    let mut result = Vec::new();
    for i in 0..count {
        let mut buf = vec![0u16; cap];
        let mut len = cap as u32;
        let rc = unsafe {
            RegEnumValueW(
                hkey,
                i,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != 0 || len as usize > buf.len() {
            continue;
        }
        let raw = String::from_utf16_lossy(&buf[..len as usize]);
        if let Some(name) = tidy(&raw) {
            result.push(name);
        }
    }
    unsafe {
        RegCloseKey(hkey);
    }
    result
}

#[cfg(target_os = "macos")]
fn from_directories() -> Vec<String> {
    use std::fs;
    use std::path::PathBuf;

    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Library/Fonts"));
    }

    let mut names = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
            if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "dfont") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !stem.is_empty() && stem.len() <= 120 {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names
}

pub fn system_fonts() -> Vec<String> {
    let mut fonts = Vec::new();
    #[cfg(windows)]
    {
        const FONTS_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
        fonts.extend(from_key(0, FONTS_KEY));
        fonts.extend(from_key(1, FONTS_KEY));
    }
    #[cfg(target_os = "macos")]
    fonts.extend(from_directories());
    fonts.extend(fallback_fonts());
    fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_contains_chinese_fonts() {
        let fonts = system_fonts();
        assert!(fonts.iter().any(|f| f == "Microsoft YaHei"));
        assert!(fonts.iter().any(|f| f == "SimSun"));
        assert!(fonts.windows(2).all(|w| w[0] != w[1]));
        #[cfg(windows)]
        assert!(tidy("Arial (TrueType)").is_some());
        #[cfg(target_os = "macos")]
        assert!(fonts.iter().any(|f| f == "PingFang SC"));
    }
}
