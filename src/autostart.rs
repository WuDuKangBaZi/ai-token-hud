use std::path::Path;

/// 按 Freedesktop Desktop Entry Exec 规范对可执行路径进行转义与加引号
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn quote_desktop_exec(exe: &Path) -> String {
    let raw = exe.to_string_lossy();
    let needs_quote = raw.is_empty()
        || raw.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | '\\' | '>' | '<' | '~' | '|' | '&' | ';' | '$' | '*' | '?' | '#' | '(' | ')' | '`'
                )
        });
    if !needs_quote {
        return raw.into_owned();
    }
    let mut out = String::with_capacity(raw.len() + 4);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' | '\\' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// 生成符合 XDG Autostart 规范的 `.desktop` 文件内容
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn build_desktop_entry(exe: &Path) -> String {
    let exec = quote_desktop_exec(exe);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=AI Token HUD\n\
         Comment=Local AI Token and Cost Telemetry HUD\n\
         Exec={exec}\n\
         Icon=ai-token-hud\n\
         Terminal=false\n\
         StartupNotify=false\n\
         Categories=Utility;Development;Monitor;\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn extract_desktop_exec(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Exec=") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn is_desktop_entry_enabled(content: &str) -> bool {
    if extract_desktop_exec(content).is_none() {
        return false;
    }
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("Hidden=true")
            || trimmed.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
        {
            return false;
        }
    }
    true
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ,
    };

    // 开机自启动：写入当前用户的 Run 注册表项 (仅 HKCU，无需管理员权限)
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "AiTokenHud";

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    /// 注册表中登记的启动命令 (路径加引号，兼容含空格的目录)
    pub(super) fn command_for(exe: &Path) -> String {
        format!("\"{}\"", exe.display())
    }

    fn current_command() -> Option<String> {
        std::env::current_exe().ok().map(|p| command_for(&p))
    }

    /// 读取已登记的启动命令，未登记时为 None
    fn registered_command() -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let ret = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                wide(VALUE_NAME).as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut size,
            )
        };
        if ret != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn is_enabled() -> bool {
        registered_command().is_some()
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let mut key: HKEY = std::ptr::null_mut();
        let ret = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) };
        if ret != ERROR_SUCCESS {
            return Err(format!("打开注册表启动项失败 (错误码 {})", ret));
        }

        let name = wide(VALUE_NAME);
        let ret = if enabled {
            match current_command() {
                Some(cmd) => {
                    let data = wide(&cmd);
                    unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32) }
                }
                None => {
                    unsafe { RegCloseKey(key) };
                    return Err("无法获取当前程序路径".to_string());
                }
            }
        } else {
            match unsafe { RegDeleteValueW(key, name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            }
        };
        unsafe { RegCloseKey(key) };

        if ret == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("写入注册表启动项失败 (错误码 {})", ret))
        }
    }

    /// 已开启自启动但 exe 被移动过时，把登记的路径更新为当前位置
    pub fn refresh_path() {
        if let (Some(registered), Some(current)) = (registered_command(), current_command()) {
            if registered != current {
                let _ = set_enabled(true);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{build_desktop_entry, extract_desktop_exec, is_desktop_entry_enabled, quote_desktop_exec};
    use std::fs;
    use std::io::ErrorKind;
    use std::path::PathBuf;

    const DESKTOP_FILE_NAME: &str = "ai-token-hud.desktop";

    fn autostart_file_path() -> Option<PathBuf> {
        let cfg_dir = dirs::config_dir().or_else(|| dirs::home_dir().map(|h| h.join(".config")))?;
        Some(cfg_dir.join("autostart").join(DESKTOP_FILE_NAME))
    }

    pub fn is_enabled() -> bool {
        let Some(path) = autostart_file_path() else {
            return false;
        };
        let Ok(content) = fs::read_to_string(path) else {
            return false;
        };
        is_desktop_entry_enabled(&content)
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let path = autostart_file_path()
            .ok_or_else(|| "无法解析 XDG autostart 配置目录".to_string())?;
        if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("无法获取当前程序路径: {}", e))?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("创建 autostart 目录失败: {}", e))?;
            }
            let content = build_desktop_entry(&exe);
            fs::write(&path, content)
                .map_err(|e| format!("写入 {} 失败: {}", path.display(), e))?;
            Ok(())
        } else {
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("删除 {} 失败: {}", path.display(), e)),
            }
        }
    }

    pub fn refresh_path() {
        let Some(path) = autostart_file_path() else {
            return;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        if !is_desktop_entry_enabled(&content) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let expected_exec = quote_desktop_exec(&exe);
        if extract_desktop_exec(&content).as_deref() != Some(expected_exec.as_str()) {
            let _ = set_enabled(true);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set_enabled(_enabled: bool) -> Result<(), String> {
        Err("当前平台暂不支持开机自启动".to_string())
    }

    pub fn refresh_path() {}
}

pub fn is_enabled() -> bool {
    imp::is_enabled()
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    imp::set_enabled(enabled)
}

pub fn refresh_path() {
    imp::refresh_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn test_command_is_quoted_for_paths_with_spaces() {
        let cmd = imp::command_for(Path::new(r"C:\Program Files\AI HUD\ai-token-hud.exe"));
        assert_eq!(cmd, r#""C:\Program Files\AI HUD\ai-token-hud.exe""#);
    }

    #[test]
    fn test_desktop_entry_formatting_and_parsing() {
        let simple = Path::new("/home/alice/.local/bin/ai-token-hud");
        assert_eq!(quote_desktop_exec(simple), "/home/alice/.local/bin/ai-token-hud");

        let with_spaces = Path::new("/opt/My Apps/ai-token-hud");
        let entry = build_desktop_entry(with_spaces);
        assert!(entry.contains("Exec=\"/opt/My Apps/ai-token-hud\""));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert_eq!(
            extract_desktop_exec(&entry).as_deref(),
            Some("\"/opt/My Apps/ai-token-hud\"")
        );
        assert!(is_desktop_entry_enabled(&entry));

        let disabled = format!("{entry}Hidden=true\n");
        assert!(!is_desktop_entry_enabled(&disabled));

        let gnome_disabled = entry.replace("X-GNOME-Autostart-enabled=true", "X-GNOME-Autostart-enabled=false");
        assert!(!is_desktop_entry_enabled(&gnome_disabled));
    }
}
