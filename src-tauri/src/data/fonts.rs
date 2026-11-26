//! Enumerate fonts installed on the machine for the three font pickers.
//! Windows keeps the human-readable family names in the registry Fonts key;
//! HKLM covers system fonts, HKCU covers per-user installed fonts.
//! macOS asks CoreText for the available family names. Reading the font
//! directories instead would hand out file stems — "STHeiti Medium" rather
//! than "Heiti SC" — which CSS cannot match, so the text silently fell back
//! to the default font.

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
fn from_core_text() -> Vec<String> {
    use core_text::font_manager::copy_available_font_family_names;

    // These are the names the system font matcher itself uses, so whatever the
    // picker shows is guaranteed to resolve in CSS.
    copy_available_font_family_names()
        .iter()
        .map(|name| name.to_string())
        .filter(|name| {
            let trimmed = name.trim();
            !trimmed.is_empty() && trimmed.chars().count() <= 120
        })
        .collect()
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
    fonts.extend(from_core_text());
    // The built-in list is a Windows safety net. On macOS every entry is a
    // font that is not installed, and picking one silently fell back to the
    // default, so it is only used when the platform returned nothing at all.
    if fonts.is_empty() {
        fonts.extend(fallback_fonts());
    }
    fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_list_contains_chinese_fonts() {
        let fonts = fallback_fonts();
        assert!(fonts.iter().any(|f| f == "Microsoft YaHei"));
        assert!(fonts.iter().any(|f| f == "SimSun"));
        #[cfg(windows)]
        assert!(tidy("Arial (TrueType)").is_some());
    }

    #[test]
    fn system_fonts_are_sorted_and_unique() {
        let fonts = system_fonts();
        assert!(!fonts.is_empty());
        assert!(fonts.windows(2).all(|w| w[0] != w[1]));
        // Helvetica ships with every macOS install and can only appear through
        // CoreText, so its presence proves the family-name path ran.
        #[cfg(target_os = "macos")]
        assert!(
            fonts.iter().any(|f| f.eq_ignore_ascii_case("helvetica")),
            "CoreText enumeration returned no Helvetica: {:?}",
            &fonts[..fonts.len().min(10)]
        );
        // Windows-only fonts must not be offered on macOS: picking them fell
        // back to the default instead of applying anything.
        #[cfg(target_os = "macos")]
        assert!(!fonts.iter().any(|f| f == "Microsoft YaHei"));
    }
}
