use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_HIDE, SW_SHOW,
};

use crate::theme::ThemeKind;

pub static OPEN_SETTINGS_FLAG: AtomicBool = AtomicBool::new(false);
/// 托盘菜单请求将 HUD 复位到屏幕顶部居中
pub static RESET_POSITION_FLAG: AtomicBool = AtomicBool::new(false);
/// 托盘菜单请求切换鼠标穿透模式 (开启穿透后用户仍可随时从托盘一键恢复点击)
pub static TOGGLE_CLICK_THROUGH_FLAG: AtomicBool = AtomicBool::new(false);
/// 托盘菜单请求一键切换双视觉主题 (Glass <-> Cyber)
pub static TOGGLE_THEME_FLAG: AtomicBool = AtomicBool::new(false);
/// 托盘菜单请求退出程序（通知主循环先行落盘缓存后退出）
pub static QUIT_APP_FLAG: AtomicBool = AtomicBool::new(false);

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
    show_item: MenuItem,
    click_through_item: MenuItem,
    theme_item: MenuItem,
    settings_item: MenuItem,
    reset_item: MenuItem,
    quit_item: MenuItem,
    pub show_item_id: MenuId,
    pub settings_item_id: MenuId,
    pub quit_item_id: MenuId,
}

impl SystemTray {
    fn format_labels(
        lang: crate::i18n::Lang,
        click_through: bool,
        theme_kind: ThemeKind,
    ) -> (String, String, String, String, String, String) {
        if lang.is_en() {
            let ct = if click_through {
                "✓ Click-Through Mode (Ctrl+Alt+T)".to_string()
            } else {
                "   Click-Through Mode (Ctrl+Alt+T)".to_string()
            };
            let th = match theme_kind {
                ThemeKind::Glass => "Switch Theme: Modern Glass → Cyber OSD".to_string(),
                ThemeKind::Cyber => "Switch Theme: Cyber OSD → Modern Glass".to_string(),
            };
            (
                "Show / Hide HUD Bar (Ctrl+Alt+H)".to_string(),
                ct,
                th,
                "Analytics & Settings Studio".to_string(),
                "Reset Position to Top Center".to_string(),
                "Quit AI Token HUD".to_string(),
            )
        } else {
            let ct = if click_through {
                "✓ 鼠标穿透模式 (Ctrl+Alt+T · 点击关闭)".to_string()
            } else {
                "   开启鼠标穿透模式 (Ctrl+Alt+T)".to_string()
            };
            let th = match theme_kind {
                ThemeKind::Glass => "切换主题：当前「极客玻璃」→ 硬核遥测".to_string(),
                ThemeKind::Cyber => "切换主题：当前「硬核遥测」→ 极客玻璃".to_string(),
            };
            (
                "显示 / 隐藏 HUD 悬浮窗 (Ctrl+Alt+H)".to_string(),
                ct,
                th,
                "打开统计与设置工作台".to_string(),
                "复位到屏幕顶部居中".to_string(),
                "退出 AI Token HUD".to_string(),
            )
        }
    }

    pub fn sync_state(&self, lang: crate::i18n::Lang, click_through: bool, theme_kind: ThemeKind) {
        let (show_l, ct_l, th_l, set_l, rst_l, quit_l) = Self::format_labels(lang, click_through, theme_kind);
        self.show_item.set_text(show_l);
        self.click_through_item.set_text(ct_l);
        self.theme_item.set_text(th_l);
        self.settings_item.set_text(set_l);
        self.reset_item.set_text(rst_l);
        self.quit_item.set_text(quit_l);
    }

    pub fn set_tooltip(&self, tip: &str) {
        let _ = self._tray.set_tooltip(Some(tip));
    }

    pub fn new(
        ctx: eframe::egui::Context,
        lang: crate::i18n::Lang,
        click_through: bool,
        theme_kind: ThemeKind,
    ) -> Option<Self> {
        // 超采样抗锯齿 32x32 仪表盘光环托盘图标
        let width = 32u32;
        let height = 32u32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= 14.8 {
                    let edge_aa = ((14.8 - dist) / 1.1).clamp(0.0, 1.0);
                    let (r, g, b) = if dist <= 5.2 {
                        // 核心脉冲青蓝
                        (96u8, 165u8, 250u8)
                    } else if dist <= 11.4 {
                        // 深曜石底盘
                        (15u8, 23u8, 42u8)
                    } else {
                        // 翡翠绿能量外环
                        (52u8, 211u8, 153u8)
                    };
                    rgba.extend_from_slice(&[r, g, b, (edge_aa * 255.0) as u8]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }

        let icon = tray_icon::Icon::from_rgba(rgba, width, height).ok()?;

        let menu = Menu::new();
        let (show_l, ct_l, th_l, set_l, rst_l, quit_l) = Self::format_labels(lang, click_through, theme_kind);
        let show_item = MenuItem::new(show_l, true, None);
        let click_through_item = MenuItem::new(ct_l, true, None);
        let theme_item = MenuItem::new(th_l, true, None);
        let settings_item = MenuItem::new(set_l, true, None);
        let reset_item = MenuItem::new(rst_l, true, None);
        let quit_item = MenuItem::new(quit_l, true, None);

        let show_item_id = show_item.id().clone();
        let ct_id = click_through_item.id().clone();
        let th_id = theme_item.id().clone();
        let settings_item_id = settings_item.id().clone();
        let reset_id = reset_item.id().clone();
        let quit_item_id = quit_item.id().clone();

        let _ = menu.append(&show_item);
        let _ = menu.append(&settings_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&click_through_item);
        let _ = menu.append(&theme_item);
        let _ = menu.append(&reset_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AI Token HUD")
            .with_icon(icon)
            .build()
            .ok()?;

        let show_id = show_item_id.clone();
        let settings_id = settings_item_id.clone();
        let quit_id = quit_item_id.clone();

        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
            if event.id == quit_id {
                QUIT_APP_FLAG.store(true, Ordering::SeqCst);
            } else if event.id == show_id {
                toggle_hud_visibility();
            } else if event.id == ct_id {
                TOGGLE_CLICK_THROUGH_FLAG.store(true, Ordering::SeqCst);
            } else if event.id == th_id {
                TOGGLE_THEME_FLAG.store(true, Ordering::SeqCst);
            } else if event.id == settings_id {
                show_hud_and_open_settings();
            } else if event.id == reset_id {
                RESET_POSITION_FLAG.store(true, Ordering::SeqCst);
                show_hud();
            }
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
            show_item,
            click_through_item,
            theme_item,
            settings_item,
            reset_item,
            quit_item,
            show_item_id,
            settings_item_id,
            quit_item_id,
        })
    }
}
