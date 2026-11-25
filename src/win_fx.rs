use crate::theme::ThemeKind;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::Mutex;

/// 缓存已应用的原生窗口扩展样式状态，仅在状态变化时触发系统调用（常规帧零开销）
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AppliedWinFx {
    pub initialized: bool,
    pub blur_active: bool,
    pub corner_mode: u32,
    pub click_through: bool,
}

#[derive(Debug, Default)]
pub struct WinFxController {
    last: AppliedWinFx,
}

impl WinFxController {
    /// 同步 HUD 主悬浮窗的圆角、背景模糊与鼠标点击穿透属性
    /// `is_compact_bar`: 仅当悬浮窗处于收起的单行横条形态时启用窗口级圆角与模糊，
    /// 展开下拉面板或右键菜单时临时切回全透明无裁剪，彻底杜绝透明间隙出现矩形毛玻璃伪影。
    pub fn sync_hud_window(
        &mut self,
        #[cfg_attr(target_os = "windows", allow(unused_variables))] ctx: &eframe::egui::Context,
        backdrop_blur: bool,
        click_through: bool,
        theme_kind: ThemeKind,
        is_compact_bar: bool,
    ) {
        if cfg!(test) {
            return;
        }
        let blur_active = backdrop_blur && is_compact_bar;
        // DWM_WINDOW_CORNER_PREFERENCE: 1 = DONOTROUND, 2 = ROUND (8px), 3 = ROUNDSMALL (4px)
        let corner_mode: u32 = if !is_compact_bar {
            1
        } else if theme_kind.is_cyber() {
            3
        } else {
            2
        };
        let effective_click_through = click_through && is_compact_bar;

        let desired = AppliedWinFx {
            initialized: true,
            blur_active,
            corner_mode,
            click_through: effective_click_through,
        };
        if self.last == desired {
            return;
        }

        #[cfg(target_os = "windows")]
        {
            if apply_win32_fx(blur_active, corner_mode, effective_click_through) {
                self.last = desired;
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            if !self.last.initialized || self.last.click_through != effective_click_through {
                ctx.send_viewport_cmd_to(
                    eframe::egui::ViewportId::ROOT,
                    eframe::egui::ViewportCommand::MousePassthrough(effective_click_through),
                );
            }
            self.last = desired;
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_win32_fx(blur_active: bool, corner_mode: u32, click_through: bool) -> bool {
    use core::ffi::c_void;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TRANSPARENT,
    };

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_NONE: u32 = 1;
    const DWMSBT_TRANSIENTWINDOW: u32 = 3; // Desktop Acrylic (Win11 22H2+ 硬件加速)

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            dwAttribute: u32,
            pvAttribute: *const c_void,
            cbAttribute: u32,
        ) -> i32;
    }

    let hwnd = crate::tray::find_hud_hwnd();
    if hwnd.is_null() {
        return false;
    }

    unsafe {
        // 1. 设置鼠标点击穿透样式 (WS_EX_TRANSPARENT)
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered_mask = (WS_EX_LAYERED | WS_EX_TRANSPARENT) as isize;
        let transparent_mask = WS_EX_TRANSPARENT as isize;
        let new_style = if click_through {
            ex_style | layered_mask
        } else {
            ex_style & !transparent_mask
        };
        if new_style != ex_style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        }

        // 2. Windows 11 原生硬件抗锯齿圆角与窗口阴影
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner_mode as *const u32).cast::<c_void>(),
            core::mem::size_of::<u32>() as u32,
        );

        // 3. 使用 Win11 22H2+ 官方公开属性 DWMWA_SYSTEMBACKDROP_TYPE (Win10 自动保持半透明玻璃底色)
        let backdrop_type: u32 = if blur_active { DWMSBT_TRANSIENTWINDOW } else { DWMSBT_NONE };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop_type as *const u32).cast::<c_void>(),
            core::mem::size_of::<u32>() as u32,
        );
    }
    true
}

/// 检测当前是否运行在 Linux Wayland 会话下
pub fn is_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// 使用系统默认文件管理器打开目录
pub fn open_in_file_manager(dir: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
}

/// 释放启动期和空闲期未使用的堆/工作集内存
#[cfg(target_os = "windows")]
pub fn trim_working_set() {
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn SetProcessWorkingSetSize(
                hProcess: isize,
                dwMinimumWorkingSetSize: usize,
                dwMaximumWorkingSetSize: usize,
            ) -> i32;
        }
        let handle = GetCurrentProcess();
        let _ = SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn trim_working_set() {
    if !cfg!(test) {
        unsafe {
            let _ = libc::malloc_trim(0);
        }
    }
}

#[cfg(not(any(target_os = "windows", all(target_os = "linux", target_env = "gnu"))))]
pub fn trim_working_set() {}

// ================= 跨平台单实例互斥锁与本地 IPC 控制 =================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCommand {
    Show,
    Hide,
    Toggle,
    ClickThrough,
    Flyout,
    Settings(usize),
}

pub fn parse_ipc_command(raw: &str) -> Option<IpcCommand> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let upper = trimmed.to_ascii_uppercase();
    if upper == "SHOW" {
        Some(IpcCommand::Show)
    } else if upper == "HIDE" {
        Some(IpcCommand::Hide)
    } else if upper == "TOGGLE" {
        Some(IpcCommand::Toggle)
    } else if upper == "CLICK_THROUGH" {
        Some(IpcCommand::ClickThrough)
    } else if upper == "FLYOUT" {
        Some(IpcCommand::Flyout)
    } else if upper == "SETTINGS" {
        Some(IpcCommand::Settings(0))
    } else if let Some(rest) = upper.strip_prefix("SETTINGS:") {
        let tab = rest.trim().parse::<usize>().ok()?.min(5);
        Some(IpcCommand::Settings(tab))
    } else {
        None
    }
}

/// 单实例互斥锁守卫，释放时自动关闭内核句柄或文件锁
pub struct SingleInstanceGuard {
    #[cfg(target_os = "windows")]
    handle: isize,
    #[cfg(target_os = "linux")]
    _lock_file: Option<std::fs::File>,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if self.handle != 0 {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CloseHandle(hObject: isize) -> i32;
            }
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self._lock_file.is_some() {
                cleanup_ipc_socket();
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn runtime_file_path(filename: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(dir.trim());
        if !dir.trim().is_empty() && p.exists() {
            return p.join(format!("ai-token-hud-{filename}"));
        }
    }
    let uid = unsafe { libc::geteuid() };
    std::env::temp_dir().join(format!("ai-token-hud-{uid}-{filename}"))
}

#[cfg(target_os = "windows")]
pub fn try_acquire_single_instance_with_cmd(_ipc_cmd: Option<&str>) -> Option<SingleInstanceGuard> {
    if cfg!(test) {
        return Some(SingleInstanceGuard { handle: 0 });
    }
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_SHOW,
    };

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(lpMutexAttributes: *const core::ffi::c_void, bInitialOwner: i32, lpName: *const u16) -> isize;
        fn GetLastError() -> u32;
        fn CloseHandle(hObject: isize) -> i32;
    }

    const ERROR_ALREADY_EXISTS: u32 = 183;

    let name: Vec<u16> = OsStr::new("Local\\AiTokenHud_SingleInstance_v1")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = CreateMutexW(core::ptr::null(), 0, name.as_ptr());
        if handle == 0 {
            return Some(SingleInstanceGuard { handle: 0 });
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            let title: Vec<u16> = OsStr::new("AI Token HUD Bar")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let hwnd = FindWindowW(core::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
            }
            return None;
        }
        Some(SingleInstanceGuard { handle })
    }
}

#[cfg(target_os = "linux")]
pub fn try_acquire_single_instance_with_cmd(ipc_cmd: Option<&str>) -> Option<SingleInstanceGuard> {
    if cfg!(test) {
        return Some(SingleInstanceGuard { _lock_file: None });
    }
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let lock_path = runtime_file_path("instance.lock");
    let Ok(mut file) = OpenOptions::new().create(true).read(true).write(true).open(&lock_path) else {
        return Some(SingleInstanceGuard { _lock_file: None });
    };

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let sock_path = runtime_file_path("ipc.sock");
        if let Ok(mut stream) = UnixStream::connect(&sock_path) {
            let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
            let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
            let msg = format!("{}\n", ipc_cmd.unwrap_or("SHOW"));
            let _ = stream.write_all(msg.as_bytes());
            let _ = stream.flush();
        }
        return None;
    }

    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = writeln!(file, "{}", std::process::id());
    Some(SingleInstanceGuard {
        _lock_file: Some(file),
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn try_acquire_single_instance_with_cmd(_ipc_cmd: Option<&str>) -> Option<SingleInstanceGuard> {
    Some(SingleInstanceGuard {})
}

/// 尝试抢占全局单实例互斥锁；若已有实例在运行，自动唤醒旧实例窗口并返回 None
#[allow(dead_code)]
pub fn try_acquire_single_instance() -> Option<SingleInstanceGuard> {
    try_acquire_single_instance_with_cmd(None)
}

#[cfg(target_os = "linux")]
pub fn start_ipc_server(ctx: eframe::egui::Context) {
    if cfg!(test) {
        return;
    }
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::Ordering;

    let sock_path = runtime_file_path("ipc.sock");
    let _ = std::fs::remove_file(&sock_path);
    let Ok(listener) = UnixListener::bind(&sock_path) else {
        return;
    };
    let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));

    let _ = std::thread::Builder::new()
        .name("ai-token-hud-ipc".to_string())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut reader = BufReader::new(conn);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    if let Some(cmd) = parse_ipc_command(&line) {
                        match cmd {
                            IpcCommand::Show => crate::tray::show_hud(),
                            IpcCommand::Hide => crate::tray::hide_hud(),
                            IpcCommand::Toggle => crate::tray::toggle_hud_visibility(),
                            IpcCommand::ClickThrough => {
                                crate::tray::TOGGLE_CLICK_THROUGH_FLAG.store(true, Ordering::SeqCst);
                                ctx.request_repaint();
                            }
                            IpcCommand::Flyout => {
                                crate::hotkeys::TOGGLE_FLYOUT_FLAG.store(true, Ordering::SeqCst);
                                ctx.request_repaint();
                            }
                            IpcCommand::Settings(tab) => {
                                crate::tray::PENDING_SETTINGS_TAB.store(tab.min(5) as i8, Ordering::SeqCst);
                                crate::tray::show_hud_and_open_settings();
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
        });
}

#[cfg(not(target_os = "linux"))]
pub fn start_ipc_server(_ctx: eframe::egui::Context) {}

#[cfg(target_os = "linux")]
pub fn cleanup_ipc_socket() {
    if !cfg!(test) {
        let _ = std::fs::remove_file(runtime_file_path("ipc.sock"));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn cleanup_ipc_socket() {}

// ================= 跨平台屏幕几何、DPI 与工作区磁吸 =================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

static CACHED_MONITOR_SIZE: Mutex<Option<[f32; 2]>> = Mutex::new(None);

pub fn update_cached_monitor_size(size: Option<eframe::egui::Vec2>) {
    if let Some(s) = size.filter(|v| v.x >= 320.0 && v.y >= 200.0 && v.x.is_finite() && v.y.is_finite()) {
        if let Ok(mut guard) = CACHED_MONITOR_SIZE.lock() {
            *guard = Some([s.x, s.y]);
        }
    }
}

#[cfg(target_os = "linux")]
fn cached_monitor_size() -> Option<[f32; 2]> {
    CACHED_MONITOR_SIZE.lock().ok().and_then(|g| *g)
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn parse_xft_dpi(resources: &str) -> Option<f32> {
    for line in resources.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Xft.dpi:") {
            if let Ok(dpi) = rest.trim().parse::<f32>() {
                if dpi.is_finite() && (48.0..=480.0).contains(&dpi) {
                    return Some((dpi / 96.0).clamp(0.5, 4.0));
                }
            }
        }
    }
    None
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn select_monitor_work_area(
    monitors: &[MonitorRect],
    workarea: Option<[i32; 4]>,
    pt_px: Option<(i32, i32)>,
    scale: f32,
) -> Option<[f32; 4]> {
    if monitors.is_empty() {
        return None;
    }
    let chosen = if let Some((px, py)) = pt_px {
        monitors
            .iter()
            .find(|m| {
                px >= m.x && px < m.x + m.width && py >= m.y && py < m.y + m.height
            })
            .or_else(|| {
                monitors.iter().min_by_key(|m| {
                    let cx = m.x + m.width / 2;
                    let cy = m.y + m.height / 2;
                    let dx = (px - cx) as i64;
                    let dy = (py - cy) as i64;
                    dx * dx + dy * dy
                })
            })
    } else {
        monitors.iter().find(|m| m.primary).or_else(|| monitors.first())
    }?;

    let mut left = chosen.x;
    let mut top = chosen.y;
    let mut right = chosen.x + chosen.width;
    let mut bottom = chosen.y + chosen.height;

    if let Some([wx, wy, ww, wh]) = workarea {
        if ww > 0 && wh > 0 {
            let il = left.max(wx);
            let it = top.max(wy);
            let ir = right.min(wx + ww);
            let ib = bottom.min(wy + wh);
            if ir - il >= 320 && ib - it >= 200 {
                left = il;
                top = it;
                right = ir;
                bottom = ib;
            }
        }
    }

    let s = scale.max(0.5);
    Some([
        left as f32 / s,
        top as f32 / s,
        right as f32 / s,
        bottom as f32 / s,
    ])
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn virtual_screen_bounds(monitors: &[MonitorRect], scale: f32) -> Option<[f32; 4]> {
    let first = monitors.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x + first.width;
    let mut max_y = first.y + first.height;
    for m in &monitors[1..] {
        min_x = min_x.min(m.x);
        min_y = min_y.min(m.y);
        max_x = max_x.max(m.x + m.width);
        max_y = max_y.max(m.y + m.height);
    }
    let s = scale.max(0.5);
    Some([
        min_x as f32 / s,
        min_y as f32 / s,
        (max_x - min_x) as f32 / s,
        (max_y - min_y) as f32 / s,
    ])
}

#[cfg(target_os = "linux")]
fn query_x11_monitors_and_workarea() -> Option<(Vec<MonitorRect>, Option<[i32; 4]>)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    use x11rb::rust_connection::RustConnection;

    let (conn, screen_num) = RustConnection::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    let root = screen.root;

    let mut monitors = Vec::new();
    if let Ok(cookie) = randr::get_monitors(&conn, root, true) {
        if let Ok(reply) = cookie.reply() {
            for m in reply.monitors {
                if m.width > 0 && m.height > 0 {
                    monitors.push(MonitorRect {
                        x: m.x as i32,
                        y: m.y as i32,
                        width: m.width as i32,
                        height: m.height as i32,
                        primary: m.primary,
                    });
                }
            }
        }
    }

    if monitors.is_empty() && screen.width_in_pixels > 0 && screen.height_in_pixels > 0 {
        monitors.push(MonitorRect {
            x: 0,
            y: 0,
            width: screen.width_in_pixels as i32,
            height: screen.height_in_pixels as i32,
            primary: true,
        });
    }

    let mut workarea = None;
    if let Ok(atom_cookie) = conn.intern_atom(false, b"_NET_WORKAREA") {
        if let Ok(atom_reply) = atom_cookie.reply() {
            if atom_reply.atom != x11rb::NONE {
                if let Ok(prop_cookie) = conn.get_property(
                    false,
                    root,
                    atom_reply.atom,
                    AtomEnum::CARDINAL,
                    0,
                    4,
                ) {
                    if let Ok(prop_reply) = prop_cookie.reply() {
                        if let Some(mut vals) = prop_reply.value32() {
                            if let (Some(x), Some(y), Some(w), Some(h)) =
                                (vals.next(), vals.next(), vals.next(), vals.next())
                            {
                                if w > 0 && h > 0 {
                                    workarea = Some([x as i32, y as i32, w as i32, h as i32]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some((monitors, workarea))
}

#[cfg(target_os = "windows")]
pub fn system_dpi_scale() -> f32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    let dpi = unsafe { GetDpiForSystem() };
    if dpi >= 96 {
        dpi as f32 / 96.0
    } else {
        1.0
    }
}

#[cfg(target_os = "linux")]
pub fn system_dpi_scale() -> f32 {
    use std::sync::OnceLock;
    static CACHED_SCALE: OnceLock<f32> = OnceLock::new();
    *CACHED_SCALE.get_or_init(|| {
        for var in ["GDK_SCALE", "QT_SCALE_FACTOR"] {
            if let Ok(v) = std::env::var(var) {
                if let Ok(s) = v.trim().parse::<f32>() {
                    if s.is_finite() && (0.5..=4.0).contains(&s) {
                        return s;
                    }
                }
            }
        }
        if let Ok((conn, screen_num)) = x11rb::rust_connection::RustConnection::connect(None) {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
            if let Some(screen) = conn.setup().roots.get(screen_num) {
                if let Ok(cookie) = conn.get_property(
                    false,
                    screen.root,
                    AtomEnum::RESOURCE_MANAGER,
                    AtomEnum::STRING,
                    0,
                    65536,
                ) {
                    if let Ok(reply) = cookie.reply() {
                        let text = String::from_utf8_lossy(&reply.value);
                        if let Some(scale) = parse_xft_dpi(&text) {
                            return scale;
                        }
                    }
                }
            }
        }
        1.0
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn system_dpi_scale() -> f32 {
    1.0
}

/// 获取主显示器逻辑宽度 (像素)
#[cfg(target_os = "windows")]
pub fn primary_screen_width() -> f32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetSystemMetrics(nIndex: i32) -> i32;
    }
    let px = unsafe { GetSystemMetrics(0) }; // SM_CXSCREEN
    let scale = system_dpi_scale();
    if px > 0 {
        (px as f32 / scale).max(640.0)
    } else {
        1920.0
    }
}

#[cfg(target_os = "linux")]
pub fn primary_screen_width() -> f32 {
    if cfg!(test) {
        return 1920.0;
    }
    let scale = system_dpi_scale().max(0.5);
    if let Some((monitors, _)) = query_x11_monitors_and_workarea() {
        if let Some(m) = monitors.iter().find(|m| m.primary).or_else(|| monitors.first()) {
            return (m.width as f32 / scale).max(640.0);
        }
    }
    cached_monitor_size().map(|[w, _]| w.max(640.0)).unwrap_or(1920.0)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn primary_screen_width() -> f32 {
    1920.0
}

/// 启动期校验保存坐标是否仍在当前虚拟多屏工作区内，副屏拔除时自动复位到主屏顶部居中
#[cfg(target_os = "windows")]
pub fn clamp_pos_to_virtual_screen(pos: [f32; 2], bar_w: f32, bar_h: f32) -> [f32; 2] {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetSystemMetrics(nIndex: i32) -> i32;
    }
    let scale = system_dpi_scale();
    let vx = unsafe { GetSystemMetrics(76) } as f32 / scale; // SM_XVIRTUALSCREEN
    let vy = unsafe { GetSystemMetrics(77) } as f32 / scale; // SM_YVIRTUALSCREEN
    let vw = unsafe { GetSystemMetrics(78) } as f32 / scale; // SM_CXVIRTUALSCREEN
    let vh = unsafe { GetSystemMetrics(79) } as f32 / scale; // SM_CYVIRTUALSCREEN

    if vw <= 0.0 || vh <= 0.0 {
        return pos;
    }
    let max_x = vx + vw;
    let max_y = vy + vh;

    // 若完全落在虚拟桌面之外 (例如外接副屏已断开)，复位至主屏顶部居中
    if pos[0] + bar_w < vx + 20.0 || pos[0] > max_x - 20.0 || pos[1] + bar_h < vy + 10.0 || pos[1] > max_y - 10.0 {
        let pw = primary_screen_width();
        return [((pw - bar_w) / 2.0).max(0.0), 8.0];
    }

    let clamped_x = pos[0].clamp(vx, (max_x - bar_w.min(vw)).max(vx));
    let clamped_y = pos[1].clamp(vy, (max_y - bar_h.min(vh)).max(vy));
    [clamped_x, clamped_y]
}

#[cfg(target_os = "linux")]
pub fn clamp_pos_to_virtual_screen(pos: [f32; 2], bar_w: f32, bar_h: f32) -> [f32; 2] {
    if cfg!(test) {
        return pos;
    }
    let scale = system_dpi_scale().max(0.5);
    let Some((monitors, _)) = query_x11_monitors_and_workarea() else {
        return pos;
    };
    let Some([vx, vy, vw, vh]) = virtual_screen_bounds(&monitors, scale) else {
        return pos;
    };
    if vw <= 0.0 || vh <= 0.0 {
        return pos;
    }
    let max_x = vx + vw;
    let max_y = vy + vh;
    if pos[0] + bar_w < vx + 20.0 || pos[0] > max_x - 20.0 || pos[1] + bar_h < vy + 10.0 || pos[1] > max_y - 10.0 {
        let pw = primary_screen_width();
        return [((pw - bar_w) / 2.0).max(0.0), 36.0];
    }
    let clamped_x = pos[0].clamp(vx, (max_x - bar_w.min(vw)).max(vx));
    let clamped_y = pos[1].clamp(vy, (max_y - bar_h.min(vh)).max(vy));
    [clamped_x, clamped_y]
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn clamp_pos_to_virtual_screen(pos: [f32; 2], _bar_w: f32, _bar_h: f32) -> [f32; 2] {
    pos
}

#[cfg(target_os = "windows")]
fn query_monitor_work_area(pt_px: (i32, i32), flags: u32, scale: f32) -> Option<[f32; 4]> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        rc_monitor: Rect,
        rc_work: Rect,
        dw_flags: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromPoint(pt: Point, dwFlags: u32) -> *mut core::ffi::c_void;
        fn GetMonitorInfoW(hMonitor: *mut core::ffi::c_void, lpmi: *mut MonitorInfo) -> i32;
    }

    let s = scale.max(0.5);
    unsafe {
        let hmon = MonitorFromPoint(Point { x: pt_px.0, y: pt_px.1 }, flags);
        if hmon.is_null() {
            return None;
        }
        let mut info = MonitorInfo {
            cb_size: core::mem::size_of::<MonitorInfo>() as u32,
            rc_monitor: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            rc_work: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            dw_flags: 0,
        };
        if GetMonitorInfoW(hmon, &mut info) == 0 {
            return None;
        }
        Some([
            info.rc_work.left as f32 / s,
            info.rc_work.top as f32 / s,
            info.rc_work.right as f32 / s,
            info.rc_work.bottom as f32 / s,
        ])
    }
}

/// 获取给定逻辑坐标所在屏幕（或主屏）的工作区 [left, top, right, bottom] (逻辑像素，已避让任务栏/顶栏)
#[cfg(target_os = "windows")]
pub fn monitor_work_area_for_pos(pos: Option<[f32; 2]>, scale: f32) -> [f32; 4] {
    if cfg!(test) {
        return [0.0, 0.0, 1920.0, 1040.0];
    }
    let s = scale.max(0.5);
    let (pt, flags) = match pos {
        Some([x, y]) => (((x * s) as i32, (y * s) as i32), 2), // MONITOR_DEFAULTTONEAREST
        None => ((0, 0), 1),                                  // MONITOR_DEFAULTTOPRIMARY
    };
    query_monitor_work_area(pt, flags, s).unwrap_or_else(|| [0.0, 0.0, primary_screen_width(), 1040.0])
}

#[cfg(target_os = "linux")]
pub fn monitor_work_area_for_pos(pos: Option<[f32; 2]>, scale: f32) -> [f32; 4] {
    if cfg!(test) {
        return [0.0, 0.0, 1920.0, 1040.0];
    }
    let s = scale.max(0.5);
    if let Some((monitors, workarea)) = query_x11_monitors_and_workarea() {
        let pt_px = pos.map(|[x, y]| ((x * s) as i32, (y * s) as i32));
        if let Some(area) = select_monitor_work_area(&monitors, workarea, pt_px, s) {
            return area;
        }
    }
    if let Some([w, h]) = cached_monitor_size() {
        return [0.0, 32.0, w, h];
    }
    [0.0, 32.0, 1920.0, 1048.0]
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn monitor_work_area_for_pos(_pos: Option<[f32; 2]>, _scale: f32) -> [f32; 4] {
    [0.0, 0.0, 1920.0, 1040.0]
}

/// 根据停靠预设与显示器工作区计算窗口左上角坐标
pub fn compute_dock_pos(dock: &str, work_area: [f32; 4], win_w: f32, win_h: f32, margin: f32) -> [f32; 2] {
    let [wl, wt, wr, wb] = work_area;
    match dock {
        "top_left" => [wl + margin, wt + margin],
        "top_right" => [(wr - win_w - margin).max(wl + margin), wt + margin],
        "bottom_left" => [wl + margin, (wb - win_h - margin).max(wt + margin)],
        "bottom_right" => [(wr - win_w - margin).max(wl + margin), (wb - win_h - margin).max(wt + margin)],
        _ => [wl + ((wr - wl - win_w) * 0.5).max(0.0), wt + margin],
    }
}

/// 将任意坐标安全钳制到当前显示器工作区内，避免尺寸变化时溢出屏幕或被任务栏遮挡
pub fn clamp_pos_to_work_area(pos: [f32; 2], work_area: [f32; 4], win_w: f32, win_h: f32, margin: f32) -> [f32; 2] {
    let [wl, wt, wr, wb] = work_area;
    let max_x = (wr - win_w - margin).max(wl + margin);
    let max_y = (wb - win_h - margin).max(wt + margin);
    [pos[0].clamp(wl + margin, max_x), pos[1].clamp(wt + margin, max_y)]
}

/// 拖拽静止后，靠近当前显示器工作区边缘 (含任务栏避让) 时自动磁吸对齐
#[cfg(target_os = "windows")]
pub fn snap_pos_to_monitor_edges(pos: [f32; 2], bar_w: f32, bar_h: f32, scale: f32, threshold: f32) -> [f32; 2] {
    if cfg!(test) {
        return pos;
    }
    let s = scale.max(0.5);
    let cx = ((pos[0] + bar_w * 0.5) * s) as i32;
    let cy = ((pos[1] + bar_h * 0.5) * s) as i32;
    let Some([wl, wt, wr, wb]) = query_monitor_work_area((cx, cy), 2, s) else {
        return pos;
    };

    snap_to_bounds(pos, [wl, wt, wr, wb], bar_w, bar_h, threshold)
}

#[cfg(target_os = "linux")]
pub fn snap_pos_to_monitor_edges(pos: [f32; 2], bar_w: f32, bar_h: f32, scale: f32, threshold: f32) -> [f32; 2] {
    if cfg!(test) {
        return pos;
    }
    let center = [pos[0] + bar_w * 0.5, pos[1] + bar_h * 0.5];
    let work = monitor_work_area_for_pos(Some(center), scale);
    snap_to_bounds(pos, work, bar_w, bar_h, threshold)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn snap_pos_to_monitor_edges(pos: [f32; 2], _bar_w: f32, _bar_h: f32, _scale: f32, _threshold: f32) -> [f32; 2] {
    pos
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn snap_to_bounds(pos: [f32; 2], work: [f32; 4], bar_w: f32, bar_h: f32, threshold: f32) -> [f32; 2] {
    let [wl, wt, wr, wb] = work;
    let mut nx = pos[0];
    let mut ny = pos[1];

    if (nx - wl).abs() <= threshold {
        nx = wl + 4.0;
    } else if ((nx + bar_w) - wr).abs() <= threshold {
        nx = (wr - bar_w - 4.0).max(wl);
    }

    if (ny - wt).abs() <= threshold {
        ny = wt + 4.0;
    } else if ((ny + bar_h) - wb).abs() <= threshold {
        ny = (wb - bar_h - 4.0).max(wt);
    }

    [nx, ny]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_dock_pos_and_work_area_clamp() {
        let work = [0.0, 0.0, 1920.0, 1040.0];
        assert_eq!(compute_dock_pos("top_left", work, 500.0, 38.0, 8.0), [8.0, 8.0]);
        assert_eq!(compute_dock_pos("top_center", work, 500.0, 38.0, 8.0), [710.0, 8.0]);
        assert_eq!(compute_dock_pos("top_right", work, 500.0, 38.0, 8.0), [1412.0, 8.0]);
        assert_eq!(compute_dock_pos("bottom_left", work, 500.0, 38.0, 8.0), [8.0, 994.0]);
        assert_eq!(compute_dock_pos("bottom_right", work, 500.0, 38.0, 8.0), [1412.0, 994.0]);

        assert_eq!(clamp_pos_to_work_area([1800.0, 1020.0], work, 500.0, 38.0, 4.0), [1416.0, 998.0]);
        assert_eq!(clamp_pos_to_work_area([-50.0, -10.0], work, 500.0, 38.0, 4.0), [4.0, 4.0]);
    }

    #[test]
    fn test_parse_xft_dpi_and_ipc_commands() {
        let xrdb = "Xcursor.size: 24\nXft.dpi:\t144\nXft.antialias: 1\n";
        assert_eq!(parse_xft_dpi(xrdb), Some(1.5));
        assert_eq!(parse_xft_dpi("Xft.dpi: 96"), Some(1.0));
        assert_eq!(parse_xft_dpi("Xft.dpi: invalid"), None);

        assert_eq!(parse_ipc_command("show\n"), Some(IpcCommand::Show));
        assert_eq!(parse_ipc_command("HIDE"), Some(IpcCommand::Hide));
        assert_eq!(parse_ipc_command("toggle"), Some(IpcCommand::Toggle));
        assert_eq!(parse_ipc_command("click_through"), Some(IpcCommand::ClickThrough));
        assert_eq!(parse_ipc_command("flyout"), Some(IpcCommand::Flyout));
        assert_eq!(parse_ipc_command("SETTINGS"), Some(IpcCommand::Settings(0)));
        assert_eq!(parse_ipc_command("SETTINGS:3"), Some(IpcCommand::Settings(3)));
        assert_eq!(parse_ipc_command("SETTINGS:99"), Some(IpcCommand::Settings(5)));
        assert_eq!(parse_ipc_command("unknown"), None);
    }

    #[test]
    fn test_select_monitor_work_area_and_virtual_bounds() {
        let monitors = vec![
            MonitorRect { x: 0, y: 0, width: 1920, height: 1080, primary: true },
            MonitorRect { x: 1920, y: 0, width: 2560, height: 1440, primary: false },
        ];
        // GNOME 顶栏 32px + 左侧 Dock 64px
        let workarea = Some([64, 32, 4416, 1048]);
        let primary = select_monitor_work_area(&monitors, workarea, None, 1.0).unwrap();
        assert_eq!(primary, [64.0, 32.0, 1920.0, 1080.0]);

        let secondary = select_monitor_work_area(&monitors, workarea, Some((2500, 300)), 2.0).unwrap();
        assert_eq!(secondary, [960.0, 16.0, 2240.0, 540.0]);

        let vbounds = virtual_screen_bounds(&monitors, 1.0).unwrap();
        assert_eq!(vbounds, [0.0, 0.0, 4480.0, 1440.0]);
    }
}
