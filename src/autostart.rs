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
fn command_for(exe: &Path) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_is_quoted_for_paths_with_spaces() {
        let cmd = command_for(Path::new(r"C:\Program Files\AI HUD\ai-token-hud.exe"));
        assert_eq!(cmd, r#""C:\Program Files\AI HUD\ai-token-hud.exe""#);
    }
}
