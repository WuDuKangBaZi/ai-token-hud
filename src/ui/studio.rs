use eframe::egui;
use egui::{Frame, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2, ViewportBuilder, ViewportId};

use crate::app::TokenHudApp;
use crate::config::HudConfig;
use crate::detector::ToolDetector;
use crate::stats::StatsEngine;
use crate::theme::{self, ThemeKind, ThemeTokens};
use crate::ui::icons::{self, Icon};
use crate::ui::{pill_badge, pill_tab, toggle_switch};

pub fn render_studio_window(app: &mut TokenHudApp, root_ctx: &egui::Context) {
    if !app.show_settings_window {
        return;
    }

    let lang = app.config.lang();
    let mut builder = ViewportBuilder::default()
        .with_title(lang.tr("AI Token HUD · 统计与设置工作台", "AI Token HUD · Analytics & Studio"))
        .with_min_inner_size([700.0, 500.0])
        .with_taskbar(true)
        .with_resizable(true);

    if app.settings_window_size.is_none() {
        builder = builder.with_inner_size([900.0, 680.0]);
    }

    root_ctx.show_viewport_immediate(
        ViewportId::from_hash_of("ai_token_hud_settings_window"),
        builder,
        |ctx, _class| {
            let theme_kind = app.theme_kind();
            theme::apply_theme(ctx, theme_kind);
            let t = theme::tokens(theme_kind);
            let lang = app.config.lang();
            let mut close_window = false;

            let cur_size = ctx.screen_rect().size();
            if cur_size.x > 120.0 && cur_size.y > 120.0 {
                app.settings_window_size = Some(cur_size);
            }

            // ================= 左侧固定导航边栏 =================
            egui::SidePanel::left("studio_left_sidebar")
                .resizable(false)
                .exact_width(198.0)
                .frame(
                    Frame::none()
                        .fill(t.sidebar_bg)
                        .stroke(Stroke::new(1.0, t.border_subtle))
                        .inner_margin(Margin::symmetric(12.0, 14.0)),
                )
                .show(ctx, |ui| {
                    ui.style_mut().interaction.selectable_labels = false;

                    // 1. 品牌与今日速览卡
                    ui.horizontal(|ui| {
                        let (logo_r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        ui.painter().circle_filled(logo_r.center(), 9.0, theme::with_alpha(t.accent, 38));
                        ui.painter().circle_stroke(logo_r.center(), 8.5, Stroke::new(1.4, t.accent));
                        ui.painter().circle_filled(logo_r.center(), 3.4, t.success);

                        ui.label(RichText::new("AI Token HUD").size(13.5).strong().color(t.text_primary));
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        pill_badge(
                            ui,
                            if theme_kind.is_cyber() { "CYBER OSD" } else { "MODERN GLASS" },
                            t.accent,
                            &t,
                        );
                        pill_badge(ui, &app.config.format_cost(app.cached_today_usd), t.cost_gold, &t);
                    });

                    ui.add_space(14.0);

                    // 2. 分组侧边导航菜单
                    ui.label(RichText::new(lang.tr("数据洞察", "ANALYTICS")).size(10.0).strong().color(t.text_dim));
                    ui.add_space(4.0);
                    for (idx, icon, label) in [
                        (0usize, Icon::Calendar, lang.tr("日历与趋势", "Calendar & Trends")),
                        (1usize, Icon::Layers, lang.tr("累计下钻透视", "Attribution")),
                        (5usize, Icon::Folder, lang.tr("项目与会话", "Projects & Sessions")),
                    ] {
                        if sidebar_nav_item(ui, app.settings_tab == idx, icon, label, &t).clicked() {
                            app.settings_tab = idx;
                            if idx == 0 {
                                app.calendar_state.refresh(&app.collector, &app.config);
                            } else if idx == 1 {
                                app.refresh_breakdown();
                            } else if idx == 5 {
                                app.refresh_project_attribution();
                            }
                        }
                    }

                    ui.add_space(12.0);
                    ui.label(RichText::new(lang.tr("系统设置", "CONFIGURATION")).size(10.0).strong().color(t.text_dim));
                    ui.add_space(4.0);
                    for (idx, icon, label) in [
                        (2usize, Icon::TagPrice, lang.tr("模型单价与汇率", "Pricing & Rates")),
                        (3usize, Icon::Sliders, lang.tr("外观与悬浮窗", "Appearance & HUD")),
                        (4usize, Icon::CpuDriver, lang.tr("数据源与工具", "Drivers & Tools")),
                    ] {
                        if sidebar_nav_item(ui, app.settings_tab == idx, icon, label, &t).clicked() {
                            app.settings_tab = idx;
                        }
                    }

                    // 3. 底部快捷切换器 (主题 / 币种 / 语言)
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                        ui.add_space(2.0);
                        ui.label(RichText::new(lang.tr("修改即时生效并自动保存", "All changes auto-save")).size(10.0).color(t.text_dim));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if pill_tab(ui, !app.config.is_cny(), "USD $", &t).clicked() {
                                app.set_currency("USD");
                            }
                            if pill_tab(ui, app.config.is_cny(), "CNY ¥", &t).clicked() {
                                app.set_currency("CNY");
                            }
                            ui.add_space(2.0);
                            if pill_tab(ui, !lang.is_en(), "中", &t).clicked() {
                                app.set_language("zh", root_ctx);
                            }
                            if pill_tab(ui, lang.is_en(), "EN", &t).clicked() {
                                app.set_language("en", root_ctx);
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if pill_tab(ui, !theme_kind.is_cyber(), lang.tr("极客玻璃", "Glass"), &t).clicked() {
                                app.set_theme(ThemeKind::Glass, root_ctx);
                            }
                            if pill_tab(ui, theme_kind.is_cyber(), lang.tr("硬核遥测", "Cyber"), &t).clicked() {
                                app.set_theme(ThemeKind::Cyber, root_ctx);
                            }
                        });
                        ui.add_space(6.0);
                        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().bottom(), Stroke::new(1.0, t.divider));
                    });
                });

            // ================= 右侧主工作区 =================
            egui::CentralPanel::default()
                .frame(Frame::none().fill(t.app_bg).inner_margin(Margin::symmetric(18.0, 14.0)))
                .show(ctx, |ui| {
                    let scroll_id = match app.settings_tab {
                        0 => "studio_analytics_scroll",
                        1 => "studio_breakdown_scroll",
                        2 => "studio_pricing_scroll",
                        3 => "studio_appearance_scroll",
                        5 => "studio_projects_scroll",
                        _ => "studio_drivers_scroll",
                    };

                    egui::ScrollArea::vertical()
                        .id_salt(scroll_id)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match app.settings_tab {
                                0 => crate::ui::analytics::render_analytics_page(app, ui, &t),
                                1 => crate::ui::analytics::render_breakdown_page(app, ui, &t),
                                2 => crate::ui::pricing_page::render_pricing_page(app, ui, root_ctx, &t),
                                3 => render_appearance_page(app, ui, root_ctx, &t),
                                5 => crate::ui::analytics::render_projects_page(app, ui, &t),
                                _ => render_drivers_page(app, ui, &t),
                            }
                        });
                });

            if ctx.input(|i| i.viewport().close_requested())
                || close_window
                || ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                close_window = true;
            }
            if close_window {
                app.show_settings_window = false;
            }
        },
    );
}

fn sidebar_nav_item(ui: &mut egui::Ui, active: bool, icon: Icon, label: &str, t: &ThemeTokens) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if active {
        ui.painter().rect(
            rect,
            Rounding::same(t.control_rounding),
            t.accent_soft,
            Stroke::new(1.0, theme::with_alpha(t.accent, 120)),
        );
        // 左侧激活竖向指示条
        let bar = Rect::from_min_max(
            Pos2::new(rect.min.x + 1.5, rect.min.y + 6.0),
            Pos2::new(rect.min.x + 4.0, rect.max.y - 6.0),
        );
        ui.painter().rect_filled(bar, Rounding::same(1.5), t.accent);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(t.control_rounding), t.row_hover);
    }

    let fg = if active {
        t.text_primary
    } else if resp.hovered() {
        t.text_secondary
    } else {
        t.text_muted
    };
    let icon_col = if active { t.accent } else { fg };
    let ir = Rect::from_center_size(Pos2::new(rect.min.x + 16.0, rect.center().y), Vec2::splat(14.0));
    icons::draw(ui.painter(), ir, icon, icon_col);

    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::prop(12.0), fg);
    ui.painter().galley(Pos2::new(ir.max.x + 8.0, rect.center().y - galley.size().y * 0.5), galley, fg);
    resp
}

/// 导航页 3：外观主题卡片预览 + 悬浮窗视觉特效与系统行为设置
fn render_appearance_page(app: &mut TokenHudApp, ui: &mut egui::Ui, root_ctx: &egui::Context, t: &ThemeTokens) {
    let lang = app.config.lang();

    ui.horizontal(|ui| {
        let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        icons::draw(ui.painter(), ir, Icon::Sparkles, t.accent);
        ui.label(RichText::new(lang.tr("双主题视觉风格切换", "Visual Design Language")).size(14.0).strong().color(t.text_primary));
    });
    ui.label(RichText::new(lang.tr("实时切换整套界面令牌、圆角几何、高光准星与波形图绘制风格", "Switch tokens, geometry, surface accents, and sparkline rendering live")).size(11.0).color(t.text_muted));
    ui.add_space(6.0);

    // 1. 双主题可视化预览卡片 (Modern Glass vs Cyber OSD)
    let cur_kind = app.theme_kind();
    let mut next_theme: Option<ThemeKind> = None;
    ui.columns(2, |cols| {
        if render_theme_preview_card(
            &mut cols[0],
            ThemeKind::Glass,
            cur_kind == ThemeKind::Glass,
            lang.tr("极客暗色玻璃 (Modern Glass)", "Modern Glass (Linear / Raycast)"),
            lang.tr("柔和圆角胶囊 · 顶部镜面微光 · 顺滑面积折线 · 克制低饱和语义色", "Soft rounded pills · Specular top highlight · Area curve sparkline"),
            t,
        )
        .clicked()
        {
            next_theme = Some(ThemeKind::Glass);
        }

        if render_theme_preview_card(
            &mut cols[1],
            ThemeKind::Cyber,
            cur_kind == ThemeKind::Cyber,
            lang.tr("硬核遥测监视器 (Cyber OSD)", "Cyber OSD (RTSS Telemetry)"),
            lang.tr("锐利工业切角 · 四角战术准星 · LED 频谱波形 · 高对比荧光读数", "Tactical corner brackets · LED spectrum bars · High-contrast telemetry"),
            t,
        )
        .clicked()
        {
            next_theme = Some(ThemeKind::Cyber);
        }
    });
    if let Some(k) = next_theme {
        app.set_theme(k, root_ctx);
    }

    ui.add_space(10.0);

    // 2. HUD 悬浮条布局与指标偏好
    let hud_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(lang.tr("HUD 横条展示与指标", "HUD Bar Metrics & Layout")).size(12.5).strong().color(t.text_primary));
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("核心展示模式:", "Display Mode:")).size(11.5).color(t.text_secondary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (mode, label) in [
                    ("cost", lang.tr("仅金额 (悬停展开)", "By Cost (Hover Flyout)")),
                    ("model", lang.tr("按模型", "By Model")),
                    ("tool", lang.tr("按工具", "By Tool")),
                ] {
                    if pill_tab(ui, app.config.view_mode == mode, label, t).clicked() {
                        app.set_view_mode(mode);
                    }
                }
            });
        });

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            ui.label(RichText::new(lang.tr("全部累计 (TOTAL) 统计范围:", "TOTAL Counter Scope:")).size(11.5).color(t.text_secondary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (scope, label) in [
                    ("all", lang.tr("全部", "All Time")),
                    ("month", lang.tr("本月", "This Month")),
                    ("week", lang.tr("本周", "This Week")),
                ] {
                    if pill_tab(ui, app.config.resolved_total_scope() == scope, label, t).clicked() {
                        app.set_total_scope(scope);
                        root_ctx.request_repaint_of(ViewportId::ROOT);
                    }
                }
            });
        });

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            ui.label(RichText::new(lang.tr("HUD 屏幕停靠位置:", "HUD Dock Position:")).size(11.5).color(t.text_secondary));
            if app.config.resolved_dock_position() == "custom" {
                pill_badge(ui, lang.tr("自由拖拽", "Custom"), t.accent, t);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (dock, label) in [
                    ("bottom_right", lang.tr("右下角", "Bottom Right")),
                    ("bottom_left", lang.tr("左下角", "Bottom Left")),
                    ("top_right", lang.tr("右上角", "Top Right")),
                    ("top_center", lang.tr("居中", "Center")),
                    ("top_left", lang.tr("左上角", "Top Left")),
                ] {
                    if pill_tab(ui, app.config.resolved_dock_position() == dock, label, t).clicked() {
                        app.set_dock_position(dock, root_ctx);
                    }
                }
            });
        });

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("展开面板高度", "Flyout Panel Height")).size(11.5).color(t.text_secondary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut h = app.config.clamped_flyout_height();
                if ui
                    .add(egui::Slider::new(&mut h, 200.0..=720.0).custom_formatter(|v, _| format!("{:.0} px", v)))
                    .changed()
                {
                    app.config.flyout_height = h;
                    app.needs_reposition = true;
                    app.config.save();
                    root_ctx.request_repaint_of(ViewportId::ROOT);
                }
                for preset in [620.0, 480.0, 330.0, 240.0] {
                    if pill_tab(ui, (app.config.clamped_flyout_height() - preset).abs() < 6.0, &format!("{:.0}", preset), t).clicked() {
                        app.config.flyout_height = preset;
                        app.needs_reposition = true;
                        app.config.save();
                        root_ctx.request_repaint_of(ViewportId::ROOT);
                    }
                }
            });
        });

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        if render_switch_line(
            ui,
            lang.tr("悬停 HUD 自动展开遥测面板", "Auto-Expand Flyout on Hover"),
            lang.tr("默认关闭以防鼠标路过误触展开；开启后在「仅金额」模式下悬停 260ms 自动弹出，关闭时可点击展开按钮或按 Ctrl+Alt+F 打开", "Off by default to prevent accidental popups on hover; when enabled, hovering 260ms in Cost mode opens the flyout (or click Expand / Ctrl+Alt+F)"),
            &mut app.config.hover_expand_flyout,
            t,
        ) {
            if !app.config.hover_expand_flyout {
                app.hover_enter_time = None;
                if !app.flyout_pinned {
                    app.cost_dropdown_open = false;
                }
            }
            app.config.save();
            root_ctx.request_repaint_of(ViewportId::ROOT);
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        if render_switch_line(
            ui,
            lang.tr("实时速率迷你波形图 (Sparkline)", "Live Throughput Sparkline"),
            lang.tr("在「速率」右侧绘制最近请求的输出速率趋势波形", "Render mini throughput curve/bars beside RATE"),
            &mut app.config.show_sparkline,
            t,
        ) {
            app.config.save();
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        if render_switch_line(
            ui,
            lang.tr("横条内联显示双币种副金额", "Inline Secondary Currency on HUD"),
            lang.tr("关闭时横条保持紧凑单币种展示，副币种仍在悬浮提示与弹出面板中完整展示", "Keep HUD compact by showing secondary currency in tooltips & flyout only"),
            &mut app.config.hud_dual_currency,
            t,
        ) {
            app.config.save();
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("HUD 背景不透明度", "HUD Surface Opacity")).size(11.5).color(t.text_secondary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Slider::new(&mut app.config.opacity, 0.40..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)))
                    .changed()
                {
                    app.config.save();
                }
            });
        });
    });
    theme::paint_surface_accents(ui.painter(), hud_card.response.rect.shrink(0.5), t);
    // 确保上方开关变动持久化
    if hud_card.response.changed() {
        app.config.save();
    }

    ui.add_space(8.0);

    // 3. 原生窗口特效与系统集成 (DWM/Compositor Blur / Click-Through / Always on Top / Autostart)
    let sys_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let sys_title = if cfg!(target_os = "windows") {
            lang.tr("Windows 原生特效与窗口行为", "Native Window Effects & Behavior")
        } else {
            lang.tr("桌面窗口特效与系统行为 (Linux X11 / Wayland)", "Desktop Window Effects & Behavior")
        };
        ui.label(RichText::new(sys_title).size(12.5).strong().color(t.text_primary));
        ui.add_space(4.0);

        let mut blur = app.config.backdrop_blur;
        let (blur_title, blur_desc) = if cfg!(target_os = "windows") {
            (
                lang.tr("Windows DWM 亚克力模糊与原生抗锯齿圆角", "Windows DWM Acrylic Blur & Hardware Rounding"),
                lang.tr("收起为单行横条时启用硬件毛玻璃背景，彻底消除底层编辑器代码透射干扰", "Applies hardware backdrop blur to prevent sharp desktop text bleed-through"),
            )
        } else {
            (
                lang.tr("透明合成器背景与窗口圆角", "Compositor Transparency & Rounded Frame"),
                lang.tr("收起为单行横条时配合桌面合成器启用半透明 HUD 表面（依赖 X11/Wayland 合成器）", "Uses desktop compositor alpha transparency on the compact HUD bar"),
            )
        };
        if render_switch_line(ui, blur_title, blur_desc, &mut blur, t) {
            app.config.backdrop_blur = blur;
            app.config.save();
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        let mut ct = app.config.click_through;
        if render_switch_line(
            ui,
            lang.tr("鼠标点击穿透模式 (Click-Through)", "Mouse Click-Through Mode (OSD Pass-Through)"),
            lang.tr("开启后鼠标点击直接穿透 HUD 落到底层 IDE/游戏；可随时从右下角托盘菜单或本开关一键关闭", "Clicks pass straight through HUD to underlying windows; restore anytime from System Tray"),
            &mut ct,
            t,
        ) {
            app.set_click_through(ct);
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        let mut aot = app.config.always_on_top;
        if render_switch_line(
            ui,
            lang.tr("HUD 悬浮窗始终置顶 (Always on Top)", "Keep HUD Always on Top"),
            lang.tr("固定在所有桌面窗口最顶层；若移出屏幕可在系统托盘点击「复位到屏幕顶部居中」", "Pin above other windows; reset position anytime from the tray menu"),
            &mut aot,
            t,
        ) {
            app.config.always_on_top = aot;
            app.config.save();
            let level = if aot { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            root_ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::WindowLevel(level));
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        let mut snap = app.config.edge_snap;
        if render_switch_line(
            ui,
            lang.tr("屏幕边缘磁吸对齐 (Edge Snap)", "Screen Edge Magnetic Snap"),
            lang.tr("拖拽停靠在显示器工作区边缘附近时自动贴齐边界（避让任务栏）", "Snaps cleanly to monitor work-area edges after drag"),
            &mut snap,
            t,
        ) {
            app.config.edge_snap = snap;
            app.config.save();
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        let mut hk = app.config.global_hotkeys_enabled;
        let hk_desc = if cfg!(target_os = "windows") {
            lang.tr("在任意编辑器或全屏应用中一键控制悬浮窗：Ctrl+Alt+H 显隐 · Ctrl+Alt+T 穿透 · Ctrl+Alt+F 固定面板", "System-wide shortcuts: Ctrl+Alt+H Show/Hide · Ctrl+Alt+T Click-Through · Ctrl+Alt+F Pin Flyout")
        } else {
            lang.tr(
                "X11 会话原生支持 Ctrl+Alt+H/T/F；Wayland 桌面请在系统快捷键中绑定 ai-token-hud --toggle / --click-through / --flyout",
                "X11 supports Ctrl+Alt+H/T/F natively; on Wayland bind system shortcuts to ai-token-hud --toggle / --click-through / --flyout",
            )
        };
        if render_switch_line(
            ui,
            lang.tr("全局快捷键 (Global Hotkeys)", "Global Hotkeys"),
            hk_desc,
            &mut hk,
            t,
        ) {
            app.set_global_hotkeys(hk, root_ctx);
        }
        if app.config.global_hotkeys_enabled {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                pill_badge(ui, &format!("Ctrl+Alt+H · {}", lang.tr("显隐", "Toggle HUD")), t.accent, t);
                pill_badge(ui, &format!("Ctrl+Alt+T · {}", lang.tr("穿透", "Click-Through")), t.success, t);
                pill_badge(ui, &format!("Ctrl+Alt+F · {}", lang.tr("固定面板", "Pin Flyout")), t.purple, t);
            });
            if crate::win_fx::is_wayland() {
                ui.label(
                    RichText::new(lang.tr(
                        "提示：检测到 Wayland 会话，安全策略禁止应用直接拦截全局键盘；可通过命令行参数触发本地 IPC 控制",
                        "Wayland detected: bind desktop shortcuts to `ai-token-hud --toggle`, `--click-through`, or `--flyout`",
                    ))
                    .size(10.2)
                    .color(t.text_muted),
                );
            }
        }

        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

        let mut auto_run = app.autostart_enabled;
        let (auto_title, auto_desc) = if cfg!(target_os = "windows") {
            (
                lang.tr("开机自动启动", "Launch at Windows Sign-In"),
                lang.tr("写入当前用户 HKCU Run 注册表项（无需管理员权限），移动 exe 后自动校正路径", "Registers in HKCU Run without admin privileges; auto-heals path if exe moves"),
            )
        } else {
            (
                lang.tr("开机自动启动 (XDG Autostart)", "Launch at Desktop Sign-In"),
                lang.tr("写入 ~/.config/autostart/ai-token-hud.desktop，移动可执行文件后自动校正 Exec 路径", "Writes ~/.config/autostart/ai-token-hud.desktop; auto-heals Exec path if binary moves"),
            )
        };
        if render_switch_line(ui, auto_title, auto_desc, &mut auto_run, t) {
            match crate::autostart::set_enabled(auto_run) {
                Ok(()) => {
                    app.autostart_enabled = auto_run;
                    app.autostart_error = None;
                }
                Err(e) => app.autostart_error = Some(e),
            }
        }
        if let Some(err) = &app.autostart_error {
            ui.label(RichText::new(err).size(10.5).color(t.danger));
        }
    });
    theme::paint_surface_accents(ui.painter(), sys_card.response.rect.shrink(0.5), t);

    ui.add_space(8.0);

    // 4. 每日/月度预算与异常消费提醒卡片
    let budget_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 5.0);
            ui.label(RichText::new(lang.tr("每日预算限额:", "Daily Budget:")).size(12.0).strong().color(t.text_primary));
            let mut budget = app.config.daily_budget_usd;
            if ui
                .add(egui::DragValue::new(&mut budget).speed(0.5).range(0.0..=100_000.0).prefix("$ ").max_decimals(2))
                .changed()
            {
                app.config.daily_budget_usd = budget;
                app.config.save();
            }
            match app.config.budget_ratio(app.cached_today_usd) {
                Some(r) => {
                    let bc = theme::budget_color(r, t);
                    theme::share_bar(ui, 110.0, 7.0, r as f32, bc, t);
                    pill_badge(ui, &format!("{:.0}%", r * 100.0), bc, t);
                }
                None => {
                    ui.label(RichText::new(lang.tr("未启用 (0 关闭)", "Off (0)")).size(11.0).color(t.text_dim));
                }
            }

            ui.add_space(8.0);
            ui.label(RichText::new(lang.tr("月度预算:", "Monthly:")).size(12.0).strong().color(t.text_primary));
            let mut m_budget = app.config.monthly_budget_usd;
            if ui
                .add(egui::DragValue::new(&mut m_budget).speed(5.0).range(0.0..=1_000_000.0).prefix("$ ").max_decimals(0))
                .changed()
            {
                app.config.monthly_budget_usd = m_budget;
                app.config.save();
            }
            if let Some(mr) = app.config.monthly_budget_ratio(app.cached_month_usd) {
                let mc = theme::budget_color(mr, t);
                theme::share_bar(ui, 100.0, 7.0, mr as f32, mc, t);
                pill_badge(ui, &format!("{:.0}%", mr * 100.0), mc, t);
            }
        });

        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(lang.tr("单次高额请求标记阈值:", "Single-Request Spike Alert:")).size(11.5).color(t.text_secondary));
            let mut spike = app.config.spike_alert_usd;
            if ui
                .add(egui::DragValue::new(&mut spike).speed(0.05).range(0.0..=1000.0).prefix("$ ").max_decimals(2))
                .changed()
            {
                app.config.spike_alert_usd = spike;
                app.config.save();
            }
            ui.label(RichText::new(lang.tr("超过该金额的请求在「实时请求流」中标红提示 (0 关闭)", "Highlights expensive calls in Live Stream (0 = off)")).size(10.5).color(t.text_muted));
        });

        ui.add_space(4.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(4.0);

        let dir = HudConfig::app_dir();
        let db_size_kb = crate::cache_db::total_db_size_bytes(&crate::native_scanner::NativeScanner::db_path()) as f64 / 1024.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(lang.tr("本地配置与 SQLite 历史归档缓存", "Config & SQLite History Archive Cache")).size(11.5).strong().color(t.text_secondary));
                ui.label(
                    RichText::new(format!(
                        "{} · {} ({} reqs · {} · SQLite {:.1} KB)",
                        dir.to_string_lossy(),
                        app.history_cache.cutoff_date,
                        app.history_cache.total_requests,
                        StatsEngine::format_compact(app.history_cache.total_tokens),
                        db_size_kb
                    ))
                    .font(theme::mono(10.2))
                    .color(t.text_muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::action_button(ui, Icon::Folder, lang.tr("打开目录", "Open Folder"), t.accent, t.input_bg, t.row_hover, t.border_subtle, t.control_rounding).clicked() {
                    crate::win_fx::open_in_file_manager(&dir);
                }
                let rescan_label = if app.is_rescanning {
                    lang.tr("扫描中...", "Scanning...")
                } else {
                    lang.tr("全量重扫", "Full Rescan")
                };
                if icons::action_button(ui, Icon::Refresh, rescan_label, t.accent, t.input_bg, t.row_hover, t.border_subtle, t.control_rounding).clicked() && !app.is_rescanning {
                    app.trigger_full_rescan(ui.ctx());
                }
            });
        });
        if let Some(status) = &app.rescan_status {
            let color = if app.is_rescanning { t.accent } else { t.success };
            ui.label(RichText::new(status).size(10.5).color(color));
        }
    });
    theme::paint_surface_accents(ui.painter(), budget_card.response.rect.shrink(0.5), t);
}

fn render_theme_preview_card(
    ui: &mut egui::Ui,
    kind: ThemeKind,
    selected: bool,
    title: &str,
    desc: &str,
    active_t: &ThemeTokens,
) -> egui::Response {
    let preview_t = theme::tokens(kind);
    let w = ui.available_width();
    let h = 114.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let border_c = if selected {
        active_t.accent
    } else if resp.hovered() {
        active_t.border_strong
    } else {
        active_t.border_subtle
    };
    let border_w = if selected { 1.6 } else { 1.0 };
    ui.painter().rect(rect, Rounding::same(active_t.card_rounding), preview_t.card_bg, Stroke::new(border_w, border_c));
    theme::paint_surface_accents(ui.painter(), rect.shrink(0.8), &preview_t);

    // 内部迷你 HUD 缩略示意图
    let mock_bar = Rect::from_min_max(
        Pos2::new(rect.min.x + 12.0, rect.min.y + 12.0),
        Pos2::new(rect.max.x - 12.0, rect.min.y + 42.0),
    );
    ui.painter().rect(
        mock_bar,
        Rounding::same(preview_t.hud_rounding),
        preview_t.hud_bg,
        Stroke::new(1.0, preview_t.border_strong),
    );
    theme::paint_surface_accents(ui.painter(), mock_bar.shrink(0.5), &preview_t);

    // 状态灯 + RATE 模拟 + 迷你波形模拟
    let dot_c = Pos2::new(mock_bar.min.x + 14.0, mock_bar.center().y);
    ui.painter().circle_filled(dot_c, 3.0, preview_t.success);
    let g1 = ui.painter().layout_no_wrap("96 T/s".to_string(), theme::mono(10.5), preview_t.success);
    ui.painter().galley(Pos2::new(dot_c.x + 8.0, mock_bar.center().y - g1.size().y * 0.5), g1, preview_t.success);

    // 右侧模拟胶囊与金额
    let g2 = ui.painter().layout_no_wrap("$24.80".to_string(), theme::mono(11.0), preview_t.cost_gold);
    ui.painter().galley(Pos2::new(mock_bar.max.x - g2.size().x - 12.0, mock_bar.center().y - g2.size().y * 0.5), g2, preview_t.cost_gold);

    // 卡片标题与描述
    let title_col = if selected { active_t.accent } else { preview_t.text_primary };
    let tg = ui.painter().layout_no_wrap(title.to_string(), theme::prop(12.2), title_col);
    ui.painter().galley(Pos2::new(rect.min.x + 12.0, mock_bar.max.y + 10.0), tg, title_col);

    if selected {
        let check_r = Rect::from_center_size(Pos2::new(rect.max.x - 18.0, mock_bar.max.y + 18.0), Vec2::splat(13.0));
        icons::draw(ui.painter(), check_r, Icon::Check, active_t.accent);
    }

    let mut desc_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
        Pos2::new(rect.min.x + 12.0, mock_bar.max.y + 30.0),
        Pos2::new(rect.max.x - 12.0, rect.max.y - 6.0),
    )));
    desc_ui.add(egui::Label::new(RichText::new(desc).size(10.5).color(preview_t.text_muted)).wrap());

    resp
}

fn render_switch_line(ui: &mut egui::Ui, title: &str, subtitle: &str, val: &mut bool, t: &ThemeTokens) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - 48.0).max(120.0);
        ui.vertical(|ui| {
            ui.set_max_width(text_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(11.8).strong().color(t.text_primary));
            ui.label(RichText::new(subtitle).size(10.5).color(t.text_muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if toggle_switch(ui, val, t).changed() {
                changed = true;
            }
        });
    });
    changed
}

/// 导航页 4：数据源驱动引擎 + 双列响应式 AI 编程工具显隐矩阵
fn render_drivers_page(app: &mut TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();

    let drv_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let active_mode = app.config.resolved_driver_mode().to_string();
        let is_cc = HudConfig::is_cc_switch_installed();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icons::draw(ui.painter(), ir, Icon::Bolt, t.accent);
            ui.label(RichText::new(lang.tr("采集驱动策略", "Telemetry Driver Strategy")).size(13.0).strong().color(t.text_primary));
            let active_badge = if active_mode == "ccswitch" {
                lang.tr("当前生效：CC Switch 数据库", "Active: CC Switch DB")
            } else {
                lang.tr("当前生效：原生会话日志扫描", "Active: Native Log Scanner")
            };
            pill_badge(ui, active_badge, t.success, t);
        });

        ui.add_space(6.0);
        let mut mode_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (code, label) in [
                ("auto", lang.tr("智能自适应 (推荐)", "Auto Detect (Recommended)")),
                ("ccswitch", lang.tr("强制 CC Switch 库", "Force CC Switch DB")),
                ("native", lang.tr("强制原生会话扫描", "Force Native Scanner")),
            ] {
                if pill_tab(ui, app.config.driver_mode == code, label, t).clicked() && app.config.driver_mode != code {
                    app.config.driver_mode = code.to_string();
                    mode_changed = true;
                }
            }
        });

        if mode_changed {
            app.collector.flush_native_cache();
            app.config.save();
            app.collector.records.clear();
            app.collector.last_sync_rowid = 0;
            app.collector.last_sync_timestamp = app.collector.today_start - 1;
            app.collector.sync(&app.config);
            app.collector.flush_native_cache();
            app.history_cache.sync_historical_if_needed(&app.collector.db_path, &app.config);
            app.period_base_cache.invalidate();
            app.refresh_stats();
            app.calendar_state.refresh(&app.collector, &app.config);
            app.refresh_breakdown();
            app.refresh_project_attribution();
        }

        ui.add_space(4.0);
        let watch_count = app.db_watcher.as_ref().map_or(0, |w| w.watched_paths.len());
        let watcher_backend = if cfg!(target_os = "linux") {
            "inotify"
        } else {
            "ReadDirectoryChangesW"
        };
        let env_txt = if lang.is_en() {
            format!(
                "CC Switch: {} · Kernel Watcher: {} directories mounted ({}, 0% idle CPU)",
                if is_cc { "Detected" } else { "Not installed" },
                watch_count,
                watcher_backend
            )
        } else {
            format!(
                "本机环境: {} · 内核文件监听: 已挂载 {} 个路径 ({} 事件唤醒，空闲 0% CPU)",
                if is_cc { "已检测到 CC Switch" } else { "未安装 CC Switch" },
                watch_count,
                watcher_backend
            )
        };
        ui.label(RichText::new(env_txt).size(10.8).color(t.text_muted));

        ui.add_space(6.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(6.0);

        let db_size_kb = crate::cache_db::total_db_size_bytes(&crate::native_scanner::NativeScanner::db_path()) as f64 / 1024.0;
        let cursor_cnt = app.collector.native_scanner.state.cursors.len();
        let daily_cnt = app.collector.native_scanner.daily_archive.entries.len();
        let sess_cnt = app.collector.native_scanner.daily_archive.session_days.len();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(lang.tr("本地 SQLite 增量缓存库 (WAL)", "Local SQLite WAL Cache")).size(11.5).strong().color(t.text_secondary));
                let meta_txt = if lang.is_en() {
                    format!(
                        "native_cache.db ({:.1} KB) · {} cursors · {} daily entries · {} session-days",
                        db_size_kb, cursor_cnt, daily_cnt, sess_cnt
                    )
                } else {
                    format!(
                        "native_cache.db ({:.1} KB) · {} 个文件游标 · {} 条日汇总 · {} 条会话明细",
                        db_size_kb, cursor_cnt, daily_cnt, sess_cnt
                    )
                };
                ui.label(RichText::new(meta_txt).font(theme::mono(10.2)).color(t.text_muted));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_label = if app.is_rescanning {
                    lang.tr("全量扫描中...", "Scanning...")
                } else {
                    lang.tr("全量重新扫描", "Full Rescan")
                };
                if icons::action_button(ui, Icon::Refresh, btn_label, t.accent, t.input_bg, t.row_hover, t.border_subtle, t.control_rounding).clicked() && !app.is_rescanning {
                    app.trigger_full_rescan(ui.ctx());
                }
            });
        });
        if let Some(status) = &app.rescan_status {
            let color = if app.is_rescanning { t.accent } else { t.success };
            ui.label(RichText::new(status).size(10.5).color(color));
        }
    });
    theme::paint_surface_accents(ui.painter(), drv_card.response.rect.shrink(0.5), t);

    ui.add_space(10.0);
    ui.label(RichText::new(lang.tr("本机 AI 编程工具监控矩阵", "Monitored AI Coding Tools")).size(13.0).strong().color(t.text_primary));
    ui.label(RichText::new(lang.tr("开启开关后，对应工具的 Token 与费用将计入 HUD 横条及下钻报表", "Enabled tools are aggregated into the HUD bar and attribution views")).size(11.0).color(t.text_muted));
    ui.add_space(6.0);

    let detected_tools = ToolDetector::scan_installed_tools();
    let mut changed = false;

    for pair in detected_tools.chunks(2) {
        ui.columns(2, |cols| {
            for (col_idx, tool) in pair.iter().enumerate() {
                let ui = &mut cols[col_idx];
                let mut visible = app.config.is_tool_visible(&tool.id);
                let tc = theme::tool_color(&tool.id);
                let path_str = tool
                    .detected_path
                    .clone()
                    .unwrap_or_else(|| lang.tr("未检测到默认会话目录", "Default session directory not found").into());

                let c_resp = theme::card_with(t).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (dot_r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                        ui.painter().circle_filled(dot_r.center(), 3.5, tc);
                        ui.label(RichText::new(&tool.name).size(12.0).strong().color(t.text_primary));
                        if tool.detected {
                            pill_badge(ui, lang.tr("已安装", "Installed"), t.success, t);
                        } else {
                            pill_badge(ui, lang.tr("未检测", "Not Found"), t.text_dim, t);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if toggle_switch(ui, &mut visible, t).changed() {
                                app.config.set_tool_visible(&tool.id, visible);
                                changed = true;
                            }
                        });
                    });
                    ui.add(egui::Label::new(RichText::new(&path_str).font(theme::mono(10.0)).color(t.text_muted)).truncate())
                        .on_hover_text(path_str);
                });
                theme::paint_surface_accents(ui.painter(), c_resp.response.rect.shrink(0.5), t);
            }
        });
        ui.add_space(5.0);
    }

    if changed {
        app.refresh_stats();
        app.calendar_state.refresh(&app.collector, &app.config);
        app.refresh_breakdown();
        app.refresh_project_attribution();
    }
}
