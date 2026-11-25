use std::sync::atomic::{AtomicBool, Ordering};

/// 全局快捷键触发：展开 / 固定 / 收起三合一遥测面板 (Ctrl+Alt+F)
pub static TOGGLE_FLYOUT_FLAG: AtomicBool = AtomicBool::new(false);

pub struct GlobalHotkeyManager {
    #[cfg(target_os = "windows")]
    thread_id: u32,
    #[cfg(target_os = "linux")]
    wakeup_tx: Option<std::os::unix::net::UnixStream>,
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

    #[cfg(target_os = "linux")]
    pub fn start(ctx: eframe::egui::Context) -> Option<Self> {
        if cfg!(test) {
            return None;
        }
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{self, ConnectionExt, GrabMode, ModMask};
        use x11rb::protocol::Event;
        use x11rb::rust_connection::RustConnection;

        let (conn, screen_num) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;

        let min_kc = conn.setup().min_keycode;
        let max_kc = conn.setup().max_keycode;
        if max_kc < min_kc {
            return None;
        }
        let count = max_kc - min_kc + 1;
        let mapping = conn.get_keyboard_mapping(min_kc, count).ok()?.reply().ok()?;
        let syms_per = mapping.keysyms_per_keycode as usize;
        if syms_per == 0 {
            return None;
        }

        let find_keycode = |lower: u32, upper: u32| -> Option<u8> {
            for idx in 0..count as usize {
                let slice = &mapping.keysyms[idx * syms_per..(idx + 1) * syms_per];
                if slice.iter().any(|&ks| ks == lower || ks == upper) {
                    return Some(min_kc + idx as u8);
                }
            }
            None
        };

        let kc_h = find_keycode(0x0068, 0x0048); // 'h' / 'H'
        let kc_t = find_keycode(0x0074, 0x0054); // 't' / 'T'
        let kc_f = find_keycode(0x0066, 0x0046); // 'f' / 'F'

        let keycodes: Vec<(u8, u8)> = [
            kc_h.map(|k| (k, 1)),
            kc_t.map(|k| (k, 2)),
            kc_f.map(|k| (k, 3)),
        ]
        .into_iter()
        .flatten()
        .collect();

        if keycodes.is_empty() {
            return None;
        }

        let (wakeup_tx, wakeup_rx) = UnixStream::pair().ok()?;

        let lock_combos = [
            ModMask::from(0u16),
            ModMask::LOCK,
            ModMask::M2,
            ModMask::LOCK | ModMask::M2,
        ];
        let base_mods = ModMask::CONTROL | ModMask::M1;

        for &(kc, _) in &keycodes {
            for &extra in &lock_combos {
                let mods = base_mods | extra;
                let _ = xproto::grab_key(
                    &conn,
                    false,
                    root,
                    u16::from(mods).into(),
                    kc,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                );
            }
        }
        let _ = conn.flush();

        std::thread::Builder::new()
            .name("ai-token-hud-hotkeys".to_string())
            .spawn(move || {
                let x11_fd = conn.stream().as_raw_fd();
                let wake_fd = wakeup_rx.as_raw_fd();
                let mut fds = [
                    libc::pollfd {
                        fd: x11_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: wake_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];

                'outer: loop {
                    while let Ok(Some(ev)) = conn.poll_for_event() {
                        if let Event::KeyPress(kp) = ev {
                            if let Some(&(_, action)) = keycodes.iter().find(|&&(k, _)| k == kp.detail) {
                                match action {
                                    1 => crate::tray::toggle_hud_visibility(),
                                    2 => {
                                        crate::tray::TOGGLE_CLICK_THROUGH_FLAG.store(true, Ordering::SeqCst);
                                        ctx.request_repaint();
                                    }
                                    3 => {
                                        TOGGLE_FLYOUT_FLAG.store(true, Ordering::SeqCst);
                                        ctx.request_repaint();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    fds[0].revents = 0;
                    fds[1].revents = 0;
                    let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                    if ret < 0 {
                        let err = std::io::Error::last_os_error();
                        if err.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        break 'outer;
                    }
                    if fds[1].revents != 0 || (fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                        break 'outer;
                    }
                }

                for &(kc, _) in &keycodes {
                    for &extra in &lock_combos {
                        let mods = base_mods | extra;
                        let _ = xproto::ungrab_key(&conn, kc, root, u16::from(mods).into());
                    }
                }
                let _ = conn.flush();
            })
            .ok()?;

        Some(Self {
            wakeup_tx: Some(wakeup_tx),
        })
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
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
        #[cfg(target_os = "linux")]
        if let Some(mut tx) = self.wakeup_tx.take() {
            use std::io::Write;
            let _ = tx.write_all(&[1u8]);
        }
    }
}

impl Drop for GlobalHotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}
