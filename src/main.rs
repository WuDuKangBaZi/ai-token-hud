#![windows_subsystem = "windows"] // 隐藏控制台黑框

mod config;
mod detector;
mod collector;
mod history_cache;
mod stats;
mod watcher;
mod native_scanner;
mod tray;
mod range_stats;
mod app;

use eframe::egui;
use app::TokenHudApp;

fn main() {
    // 游戏加加风格微型横条悬浮窗配置 (高 36px，初始宽 640px，自适应多工具扩充)
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AI Token HUD Bar")
            .with_inner_size([640.0, 36.0])
            .with_min_inner_size([420.0, 36.0])
            .with_max_inner_size([3840.0, 900.0])
            .with_decorations(false)        // 无边框
            .with_transparent(true)        // 透明磨砂
            .with_always_on_top()          // 类似游戏 OSD 置顶常驻
            .with_taskbar(false)           // 不在任务栏显示图标
            .with_resizable(true),         // 允许根据展示工具数量或金额下拉自动伸缩尺寸
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "AI Token HUD Bar",
        native_options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            
            // 解决中文乱码：加载 Windows 官方系统字体 (微软雅黑 / 黑体)
            let font_candidates = [
                "C:\\Windows\\Fonts\\msyh.ttc",
                "C:\\Windows\\Fonts\\msyh.ttf",
                "C:\\Windows\\Fonts\\simhei.ttf",
                "C:\\Windows\\Fonts\\simsun.ttc",
            ];

            for path in font_candidates {
                if let Ok(font_bytes) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "system_cjk".to_owned(),
                        egui::FontData::from_owned(font_bytes).into(),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, "system_cjk".to_owned());
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .insert(0, "system_cjk".to_owned());
                    break;
                }
            }

            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(TokenHudApp::new(&cc.egui_ctx)))
        }),
    ) {
        eprintln!("AI Token HUD exited: {:?}", e);
    }
}
