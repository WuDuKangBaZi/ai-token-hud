use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem, MenuId};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_HIDE, SW_SHOW,
};

pub static OPEN_SETTINGS_FLAG: AtomicBool = AtomicBool::new(false);
/// 托盘菜单请求将 HUD 复位到屏幕顶部居中 (横条被拖出屏幕或显示器变更后找回)
pub static RESET_POSITION_FLAG: AtomicBool = AtomicBool::new(false);

pub fn find_hud_hwnd() -> HWND {
    let title: Vec<u16> = OsStr::new("AI Token HUD Bar")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if !hwnd.is_null() {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid == std::process::id() {
            return hwnd;
        }
    }
    std::ptr::null_mut()
}

#[allow(dead_code)]
pub fn is_hud_visible() -> bool {
    let hwnd = find_hud_hwnd();
    if !hwnd.is_null() {
        unsafe { IsWindowVisible(hwnd) != 0 }
    } else {
        true
    }
}

pub fn hide_hud() {
    let hwnd = find_hud_hwnd();
    if !hwnd.is_null() {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn show_hud() {
    let hwnd = find_hud_hwnd();
    if !hwnd.is_null() {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn toggle_hud_visibility() {
    let hwnd = find_hud_hwnd();
    if !hwnd.is_null() {
        unsafe {
            let visible = IsWindowVisible(hwnd) != 0;
            if visible {
                ShowWindow(hwnd, SW_HIDE);
            } else {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
            }
        }
    }
}

pub fn show_hud_and_open_settings() {
    OPEN_SETTINGS_FLAG.store(true, Ordering::SeqCst);
    show_hud();
}

#[allow(dead_code)]
pub struct SystemTray {
    _tray: TrayIcon,
    pub show_item_id: MenuId,
    pub settings_item_id: MenuId,
    pub quit_item_id: MenuId,
}

impl SystemTray {
    pub fn new(ctx: eframe::egui::Context) -> Option<Self> {
        // 生成 32x32 程序内置高科技感托盘图标 (外环深灰，中心科技天蓝)
        let width = 32;
        let height = 32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= 14.0 {
                    if dist <= 7.0 {
                        // 中心科技蓝 (56, 189, 248)
                        rgba.extend_from_slice(&[56, 189, 248, 255]);
                    } else if dist <= 12.0 {
                        // 环形深蓝灰
                        rgba.extend_from_slice(&[30, 41, 59, 255]);
                    } else {
                        // 边框亮青
                        rgba.extend_from_slice(&[74, 222, 128, 255]);
                    }
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }

        let icon = tray_icon::Icon::from_rgba(rgba, width, height).ok()?;

        let menu = Menu::new();
        let show_item = MenuItem::new("🖥️ 显示 / 隐藏悬浮窗", true, None);
        let settings_item = MenuItem::new("⚙️ 设置与详细统计", true, None);
        let reset_item = MenuItem::new("📍 复位到屏幕顶部居中", true, None);
        let quit_item = MenuItem::new("🚪 退出 AI Token HUD", true, None);

        let show_item_id = show_item.id().clone();
        let settings_item_id = settings_item.id().clone();
        let reset_id = reset_item.id().clone();
        let quit_item_id = quit_item.id().clone();

        let _ = menu.append(&show_item);
        let _ = menu.append(&settings_item);
        let _ = menu.append(&reset_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AI Token HUD")
            .with_icon(icon)
            .build()
            .ok()?;

        // 设置直接事件处理器 (直接响应 Win32 消息，毫秒级响应，即使窗口隐藏或休眠也绝不卡顿)
        let show_id = show_item_id.clone();
        let settings_id = settings_item_id.clone();
        let quit_id = quit_item_id.clone();

        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
            if event.id == quit_id {
                std::process::exit(0);
            } else if event.id == show_id {
                toggle_hud_visibility();
            } else if event.id == settings_id {
                show_hud_and_open_settings();
            } else if event.id == reset_id {
                RESET_POSITION_FLAG.store(true, Ordering::SeqCst);
                show_hud();
            }
            // 窗口已可见时 ShowWindow 不会触发重绘，主动唤醒以立即消费上面的标记
            ctx.request_repaint();
        }));

        tray_icon::TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            match event {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
                    toggle_hud_visibility();
                }
                _ => {}
            }
        }));

        Some(Self {
            _tray: tray,
            show_item_id,
            settings_item_id,
            quit_item_id,
        })
    }
}
