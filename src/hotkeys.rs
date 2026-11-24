use std::sync::atomic::{AtomicBool, Ordering};

/// 全局快捷键触发：展开 / 固定 / 收起三合一遥测面板 (Ctrl+Alt+F)
pub static TOGGLE_FLYOUT_FLAG: AtomicBool = AtomicBool::new(false);

pub struct GlobalHotkeyManager {
    #[allow(dead_code)]
    thread_id: u32,
}

impl GlobalHotkeyManager {
    #[cfg(target_os = "windows")]
    pub fn start(ctx: eframe::egui::Context) -> Option<Self> {
        if cfg!(test) {
            return None;
        }

        #[repr(C)]
        struct Point {
            x: i32,
            y: i32,
        }
        #[repr(C)]
        struct Msg {
            hwnd: *mut core::ffi::c_void,
            message: u32,
            w_param: usize,
            l_param: isize,
            time: u32,
            pt: Point,
        }

        #[link(name = "user32")]
        unsafe extern "system" {
            fn RegisterHotKey(hWnd: *mut core::ffi::c_void, id: i32, fsModifiers: u32, vk: u32) -> i32;
            fn UnregisterHotKey(hWnd: *mut core::ffi::c_void, id: i32) -> i32;
            fn GetMessageW(lpMsg: *mut Msg, hWnd: *mut core::ffi::c_void, wMsgFilterMin: u32, wMsgFilterMax: u32) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }

        const MOD_ALT: u32 = 0x0001;
        const MOD_CONTROL: u32 = 0x0002;
        const MOD_NOREPEAT: u32 = 0x4000;
        const WM_HOTKEY: u32 = 0x0312;

        const HK_TOGGLE_SHOW: i32 = 1; // Ctrl + Alt + H
        const HK_CLICK_THROUGH: i32 = 2; // Ctrl + Alt + T
        const HK_TOGGLE_FLYOUT: i32 = 3; // Ctrl + Alt + F

        let (tid_tx, tid_rx) = std::sync::mpsc::channel::<u32>();

        std::thread::Builder::new()
            .name("ai-token-hud-hotkeys".to_string())
            .spawn(move || unsafe {
                let tid = GetCurrentThreadId();
                let _ = tid_tx.send(tid);

                let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
                let _ = RegisterHotKey(core::ptr::null_mut(), HK_TOGGLE_SHOW, mods, 0x48); // 'H'
                let _ = RegisterHotKey(core::ptr::null_mut(), HK_CLICK_THROUGH, mods, 0x54); // 'T'
                let _ = RegisterHotKey(core::ptr::null_mut(), HK_TOGGLE_FLYOUT, mods, 0x46); // 'F'

                let mut msg = core::mem::zeroed::<Msg>();
                while GetMessageW(&mut msg, core::ptr::null_mut(), 0, 0) > 0 {
                    if msg.message == WM_HOTKEY {
                        match msg.w_param as i32 {
                            HK_TOGGLE_SHOW => {
                                crate::tray::toggle_hud_visibility();
                            }
                            HK_CLICK_THROUGH => {
                                crate::tray::TOGGLE_CLICK_THROUGH_FLAG.store(true, Ordering::SeqCst);
                                ctx.request_repaint();
                            }
                            HK_TOGGLE_FLYOUT => {
                                TOGGLE_FLYOUT_FLAG.store(true, Ordering::SeqCst);
                                ctx.request_repaint();
                            }
                            _ => {}
                        }
                    }
                }

                let _ = UnregisterHotKey(core::ptr::null_mut(), HK_TOGGLE_SHOW);
                let _ = UnregisterHotKey(core::ptr::null_mut(), HK_CLICK_THROUGH);
                let _ = UnregisterHotKey(core::ptr::null_mut(), HK_TOGGLE_FLYOUT);
            })
            .ok()?;

        tid_rx.recv().ok().map(|thread_id| Self { thread_id })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn start(_ctx: eframe::egui::Context) -> Option<Self> {
        None
    }

    pub fn stop(&mut self) {
        #[cfg(target_os = "windows")]
        if self.thread_id != 0 {
            #[link(name = "user32")]
            unsafe extern "system" {
                fn PostThreadMessageW(idThread: u32, Msg: u32, wParam: usize, lParam: isize) -> i32;
            }
            const WM_QUIT: u32 = 0x0012;
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            self.thread_id = 0;
        }
    }
}

impl Drop for GlobalHotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}
