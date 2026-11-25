#![cfg_attr(target_os = "windows", windows_subsystem = "windows")] // 隐藏 Windows 控制台黑框

mod config;
mod i18n;
mod pricing_catalog;
mod detector;
mod collector;
mod history_cache;
mod stats;
mod watcher;
mod cache_db;
mod native_scanner;
mod tray;
mod range_stats;
mod theme;
mod autostart;
mod win_fx;
mod hotkeys;
mod http_client;
mod project_stats;
mod ui;
mod app;

use eframe::egui;
use app::TokenHudApp;
use theme::ThemeKind;

#[cfg(target_os = "linux")]
fn fc_match_font(pattern: &str) -> Option<String> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn load_first_font(paths: &[String]) -> Option<Vec<u8>> {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    #[cfg(target_os = "windows")]
    let (ui_latin_candidates, mono_candidates, cjk_candidates) = (
        vec![
            "C:\\Windows\\Fonts\\segoeui.ttf".to_string(),
            "C:\\Windows\\Fonts\\arial.ttf".to_string(),
        ],
        vec![
            "C:\\Windows\\Fonts\\CascadiaMono.ttf".to_string(),
            "C:\\Windows\\Fonts\\CascadiaCode.ttf".to_string(),
            "C:\\Windows\\Fonts\\consola.ttf".to_string(),
        ],
        vec![
            "C:\\Windows\\Fonts\\msyh.ttc".to_string(),
            "C:\\Windows\\Fonts\\msyh.ttf".to_string(),
            "C:\\Windows\\Fonts\\simhei.ttf".to_string(),
            "C:\\Windows\\Fonts\\simsun.ttc".to_string(),
        ],
    );

    #[cfg(target_os = "linux")]
    let (ui_latin_candidates, mono_candidates, cjk_candidates) = {
        let mut latin = vec![
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf".to_string(),
            "/usr/share/fonts/truetype/ubuntu/UbuntuSans-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf".to_string(),
        ];
        if let Some(p) = fc_match_font("sans-serif") {
            latin.push(p);
        }

        let mut mono = vec![
            "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf".to_string(),
            "/usr/share/fonts/truetype/ubuntu/UbuntuSansMono-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/firacode/FiraCode-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf".to_string(),
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string(),
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf".to_string(),
        ];
        if let Some(p) = fc_match_font("monospace") {
            mono.push(p);
        }

        let mut cjk = vec![
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_string(),
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_string(),
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".to_string(),
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc".to_string(),
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf".to_string(),
            "/usr/share/fonts/truetype/arphic/uming.ttc".to_string(),
        ];
        if let Some(p) = fc_match_font(":lang=zh-cn") {
            cjk.push(p);
        }

        (latin, mono, cjk)
    };

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let (ui_latin_candidates, mono_candidates, cjk_candidates): (Vec<String>, Vec<String>, Vec<String>) =
        (Vec::new(), Vec::new(), Vec::new());

    // 1. 界面西文与标点优先采用系统原生清晰 UI 字体
    if let Some(bytes) = load_first_font(&ui_latin_candidates) {
        fonts.font_data.insert("sys_ui_latin".to_owned(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "sys_ui_latin".to_owned());
    }

    // 2. 数值与代码等宽字体栈：确保 0-9 每一位严格等宽对齐、实时跳动零抖动
    if let Some(bytes) = load_first_font(&mono_candidates) {
        fonts.font_data.insert("sys_tabular_mono".to_owned(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "sys_tabular_mono".to_owned());
    }

    // 3. 中日韩 (CJK) 字体回退：跟在西文/等宽字体之后，兼顾锐利数字与清晰中文
    if let Some(bytes) = load_first_font(&cjk_candidates) {
        fonts.font_data.insert("system_cjk".to_owned(), egui::FontData::from_owned(bytes).into());
        let prop_list = fonts.families.entry(egui::FontFamily::Proportional).or_default();
        let insert_idx = if prop_list.first().is_some_and(|n| n == "sys_ui_latin") { 1 } else { 0 };
        prop_list.insert(insert_idx, "system_cjk".to_owned());

        let mono_list = fonts.families.entry(egui::FontFamily::Monospace).or_default();
        let mono_idx = if mono_list.first().is_some_and(|n| n == "sys_tabular_mono") { 1 } else { 0 };
        mono_list.insert(mono_idx, "system_cjk".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn cli_ipc_command(args: &[String]) -> Option<String> {
    if args.iter().any(|a| a == "--toggle") {
        return Some("TOGGLE".to_string());
    }
    if args.iter().any(|a| a == "--click-through") {
        return Some("CLICK_THROUGH".to_string());
    }
    if args.iter().any(|a| a == "--flyout") {
        return Some("FLYOUT".to_string());
    }
    if args.iter().any(|a| a == "--show") {
        return Some("SHOW".to_string());
    }
    if args.iter().any(|a| a == "--hide") {
        return Some("HIDE".to_string());
    }
    if let Some(i) = args.iter().position(|a| a == "--settings") {
        let tab = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0).min(5);
        return Some(format!("SETTINGS:{tab}"));
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ipc_cmd = cli_ipc_command(&args);

    let _single_instance = match win_fx::try_acquire_single_instance_with_cmd(ipc_cmd.as_deref()) {
        Some(g) => g,
        None => return,
    };
    if ipc_cmd.as_deref() == Some("HIDE") {
        return;
    }

    let config = config::HudConfig::load();
    let (icon_rgba, icon_w, icon_h) = tray::build_app_icon_rgba();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AI Token HUD Bar")
        .with_app_id("ai-token-hud")
        .with_icon(std::sync::Arc::new(egui::IconData {
            rgba: icon_rgba,
            width: icon_w,
            height: icon_h,
        }))
        .with_inner_size([560.0, 38.0])
        .with_min_inner_size([360.0, 38.0])
        .with_max_inner_size([3840.0, 920.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_taskbar(false)
        .with_resizable(true);
    if config.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    let dock = config.resolved_dock_position();
    if dock != "custom" {
        let work = win_fx::monitor_work_area_for_pos(config.hud_pos, win_fx::system_dpi_scale());
        let init_pos = win_fx::compute_dock_pos(dock, work, 560.0, 38.0, 8.0);
        viewport = viewport.with_position(init_pos);
    } else if let Some(pos) = config.hud_pos {
        let safe_pos = win_fx::clamp_pos_to_virtual_screen(pos, 560.0, 38.0);
        viewport = viewport.with_position(safe_pos);
    }
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "AI Token HUD Bar",
        native_options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            theme::apply_theme(&cc.egui_ctx, ThemeKind::from_code(&config.ui_theme));
            Ok(Box::new(TokenHudApp::new(&cc.egui_ctx)))
        }),
    ) {
        eprintln!("AI Token HUD exited: {:?}", e);
    }
}
