#![windows_subsystem = "windows"] // 隐藏控制台黑框

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

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. 界面西文与标点优先采用 Windows 11/10 原生清晰 UI 字体 (Segoe UI)
    let ui_latin_candidates = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];
    for path in ui_latin_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("win_ui_latin".to_owned(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "win_ui_latin".to_owned());
            break;
        }
    }

    // 2. 数值与代码等宽字体栈：确保 0-9 每一位严格等宽对齐、实时跳动零抖动
    let mono_candidates = [
        "C:\\Windows\\Fonts\\CascadiaMono.ttf",
        "C:\\Windows\\Fonts\\CascadiaCode.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
    ];
    for path in mono_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("win_tabular_mono".to_owned(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "win_tabular_mono".to_owned());
            break;
        }
    }

    // 3. 中日韩 (CJK) 字体回退：跟在西文/等宽字体之后，兼顾锐利数字与清晰中文
    let cjk_candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];
    for path in cjk_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("system_cjk".to_owned(), egui::FontData::from_owned(bytes).into());
            let prop_list = fonts.families.entry(egui::FontFamily::Proportional).or_default();
            let insert_idx = if prop_list.first().is_some_and(|n| n == "win_ui_latin") { 1 } else { 0 };
            prop_list.insert(insert_idx, "system_cjk".to_owned());

            let mono_list = fonts.families.entry(egui::FontFamily::Monospace).or_default();
            let mono_idx = if mono_list.first().is_some_and(|n| n == "win_tabular_mono") { 1 } else { 0 };
            mono_list.insert(mono_idx, "system_cjk".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}

fn main() {
    let _single_instance = match win_fx::try_acquire_single_instance() {
        Some(g) => g,
        None => return,
    };

    let config = config::HudConfig::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AI Token HUD Bar")
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
