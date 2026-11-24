use crate::theme::ThemeKind;

/// 缓存已应用的原生窗口扩展样式状态，仅在状态变化时触发 Win32/DWM 系统调用（常规帧零开销）
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
    /// 同步 HUD 主悬浮窗的 DWM 圆角、系统级亚克力背景模糊与鼠标点击穿透属性
    /// `is_compact_bar`: 仅当悬浮窗处于收起的单行横条形态时启用 DWM 窗口级圆角与模糊，
    /// 展开下拉面板或右键菜单时临时切回全透明无裁剪，彻底杜绝透明间隙出现矩形毛玻璃伪影。
    pub fn sync_hud_window(
        &mut self,
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
        if apply_win32_fx(blur_active, corner_mode, effective_click_through) {
            self.last = desired;
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_win32_fx(blur_active: bool, corner_mode: u32, click_through: bool) -> bool {
    use core::ffi::c_void;

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_LAYERED: isize = 0x0008_0000;
    const WS_EX_TRANSPARENT: isize = 0x0000_0020;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_NONE: u32 = 1;
    const DWMSBT_TRANSIENTWINDOW: u32 = 3; // Desktop Acrylic (Win11 22H2+ 硬件加速无拖拽延迟)

    #[repr(C)]
    struct AccentPolicy {
        accent_state: i32,
        accent_flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }

    #[repr(C)]
    struct WindowCompositionAttribData {
        attrib: u32,
        pv_data: *mut c_void,
        cb_data: usize,
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            dwAttribute: u32,
            pvAttribute: *const c_void,
            cbAttribute: u32,
        ) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowLongPtrW(hwnd: *mut c_void, nIndex: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: *mut c_void, nIndex: i32, dwNewLong: isize) -> isize;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleA(lpModuleName: *const u8) -> *mut c_void;
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
    }

    let hwnd = crate::tray::find_hud_hwnd();
    if hwnd.is_null() {
        return false;
    }

    unsafe {
        // 1. 设置鼠标点击穿透样式 (WS_EX_TRANSPARENT)
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = if click_through {
            ex_style | WS_EX_LAYERED | WS_EX_TRANSPARENT
        } else {
            ex_style & !WS_EX_TRANSPARENT
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

        // 3. 优先使用 Win11 22H2+ DWM Transient Acrylic，若系统不支持则平滑回退 Win10 BlurBehind
        let backdrop_type: u32 = if blur_active { DWMSBT_TRANSIENTWINDOW } else { DWMSBT_NONE };
        let hr = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop_type as *const u32).cast::<c_void>(),
            core::mem::size_of::<u32>() as u32,
        );

        if hr != 0 {
            let user32 = GetModuleHandleA(b"user32.dll\0".as_ptr());
            if !user32.is_null() {
                let proc_ptr = GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr());
                if !proc_ptr.is_null() {
                    type SetWcaFn = unsafe extern "system" fn(*mut c_void, *mut WindowCompositionAttribData) -> i32;
                    let set_wca: SetWcaFn = core::mem::transmute(proc_ptr);
                    // ACCENT_ENABLE_BLURBEHIND = 3 (在 Win10 下无拖拽卡顿), ACCENT_DISABLED = 0
                    let mut policy = AccentPolicy {
                        accent_state: if blur_active { 3 } else { 0 },
                        accent_flags: 0,
                        gradient_color: 0,
                        animation_id: 0,
                    };
                    let mut data = WindowCompositionAttribData {
                        attrib: 19, // WCA_ACCENT_POLICY
                        pv_data: (&mut policy as *mut AccentPolicy).cast::<c_void>(),
                        cb_data: core::mem::size_of::<AccentPolicy>(),
                    };
                    let _ = set_wca(hwnd, &mut data);
                }
            }
        }
    }
    true
}

#[cfg(not(target_os = "windows"))]
fn apply_win32_fx(_blur_active: bool, _corner_mode: u32, _click_through: bool) -> bool {
    true
}

/// 在 Windows 下释放启动期和空闲期未使用的显卡驱动和字体初始化工作集内存
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

#[cfg(not(target_os = "windows"))]
pub fn trim_working_set() {}

/// Win32 单实例互斥锁守卫，释放时自动关闭内核句柄
pub struct SingleInstanceGuard {
    handle: isize,
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
    }
}

/// 尝试抢占全局单实例互斥锁；若已有实例在运行，自动唤醒旧实例窗口并返回 None
#[cfg(target_os = "windows")]
pub fn try_acquire_single_instance() -> Option<SingleInstanceGuard> {
    if cfg!(test) {
        return Some(SingleInstanceGuard { handle: 0 });
    }
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(lpMutexAttributes: *const core::ffi::c_void, bInitialOwner: i32, lpName: *const u16) -> isize;
        fn GetLastError() -> u32;
        fn CloseHandle(hObject: isize) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> *mut core::ffi::c_void;
        fn ShowWindow(hWnd: *mut core::ffi::c_void, nCmdShow: i32) -> i32;
        fn SetForegroundWindow(hWnd: *mut core::ffi::c_void) -> i32;
    }

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const SW_SHOW: i32 = 5;

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

#[cfg(not(target_os = "windows"))]
pub fn try_acquire_single_instance() -> Option<SingleInstanceGuard> {
    Some(SingleInstanceGuard { handle: 0 })
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

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
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

/// 获取给定逻辑坐标所在屏幕（或主屏）的工作区 [left, top, right, bottom] (逻辑像素，已避让任务栏)
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

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
pub fn snap_pos_to_monitor_edges(pos: [f32; 2], _bar_w: f32, _bar_h: f32, _scale: f32, _threshold: f32) -> [f32; 2] {
    pos
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
}
