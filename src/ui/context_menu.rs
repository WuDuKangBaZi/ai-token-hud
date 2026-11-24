use eframe::egui;
use egui::{Color32, Frame, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::TokenHudApp;
use crate::theme::{self, ThemeKind, ThemeTokens};
use crate::ui::hud_bar::HUD_BAR_HEIGHT;
use crate::ui::icons::{self, Icon};
use crate::ui::pill_tab;

pub const CONTEXT_MENU_VIEWPORT_HEIGHT: f32 = 340.0;

pub fn render_context_menu(app: &mut TokenHudApp, ctx: &egui::Context, t: &ThemeTokens) {
    let lang = app.config.lang();
    let menu_w = 276.0;
    let is_bottom = app.is_bottom_dock();
    let bar_rect = app.hud_bar_rect(ctx.screen_rect());
    let target_pos = if let Some(pos) = app.context_menu_pos {
        let x = pos.x.min(app.current_bar_width - menu_w - 4.0).max(4.0);
        let y = if is_bottom {
            4.0
        } else {
            (pos.y + 4.0).max(HUD_BAR_HEIGHT + 2.0)
        };
        Pos2::new(x, y)
    } else {
        let y = if is_bottom { 4.0 } else { HUD_BAR_HEIGHT + 3.0 };
        Pos2::new(
            (bar_rect.max.x - menu_w).clamp(4.0, (app.current_bar_width - menu_w - 4.0).max(4.0)),
            y,
        )
    };

    let menu_response = egui::Area::new(egui::Id::new("hud_context_menu_v2"))
        .fixed_pos(target_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::none()
                .fill(t.flyout_bg)
                .stroke(Stroke::new(1.0, t.border_strong))
                .rounding(Rounding::same(t.flyout_rounding))
                .inner_margin(Margin::symmetric(10.0, 9.0))
                .show(ui, |ui| {
                    ui.set_min_width(menu_w - 20.0);
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 5.0);

                    // 1. 展示模式 + 币种 + 语言 胶囊条
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (mode, label) in [
                            ("tool", lang.tr("工具", "Tool")),
                            ("model", lang.tr("模型", "Model")),
                            ("cost", lang.tr("金额", "Cost")),
                        ] {
                            if pill_tab(ui, app.config.view_mode == mode, label, t).clicked() {
                                app.set_view_mode(mode);
                            }
                        }
                        ui.add_space(2.0);
                        theme::vdiv_with(ui, t, 14.0);
                        ui.add_space(2.0);
                        for (cur, label) in [("USD", "$"), ("CNY", "¥")] {
                            if pill_tab(ui, app.config.currency == cur, label, t).clicked() {
                                app.set_currency(cur);
                            }
                        }
                        ui.add_space(2.0);
                        theme::vdiv_with(ui, t, 14.0);
                        ui.add_space(2.0);
                        for (code, label) in [("zh", "中"), ("en", "EN")] {
                            if pill_tab(ui, lang.as_code() == code, label, t).clicked() {
                                app.set_language(code, ctx);
                            }
                        }
                    });

                    // 2. 双主题实时切换行 (极客玻璃 vs 硬核遥测)
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(RichText::new(lang.tr("主题:", "Theme:")).size(11.0).color(t.text_muted));
                        if pill_tab(ui, !t.kind.is_cyber(), lang.tr("极客玻璃", "Modern Glass"), t).clicked() {
                            app.set_theme(ThemeKind::Glass, ctx);
                        }
                        if pill_tab(ui, t.kind.is_cyber(), lang.tr("硬核遥测", "Cyber OSD"), t).clicked() {
                            app.set_theme(ThemeKind::Cyber, ctx);
                        }
                    });

                    // 3. TOTAL 统计范围快速切换行
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.label(RichText::new(lang.tr("累计:", "Scope:")).size(11.0).color(t.text_muted));
                        for (scope, label) in [
                            ("week", lang.tr("本周", "Week")),
                            ("month", lang.tr("本月", "Month")),
                            ("all", lang.tr("全部", "All")),
                        ] {
                            if pill_tab(ui, app.config.resolved_total_scope() == scope, label, t).clicked() {
                                app.set_total_scope(scope);
                            }
                        }
                    });

                    // 4. 屏幕停靠预设行
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.5;
                        ui.label(RichText::new(lang.tr("停靠:", "Dock:")).size(11.0).color(t.text_muted));
                        for (dock, label) in [
                            ("top_left", lang.tr("左上", "TL")),
                            ("top_center", lang.tr("居中", "Top")),
                            ("top_right", lang.tr("右上", "TR")),
                            ("bottom_left", lang.tr("左下", "BL")),
                            ("bottom_right", lang.tr("右下", "BR")),
                        ] {
                            if pill_tab(ui, app.config.resolved_dock_position() == dock, label, t).clicked() {
                                app.set_dock_position(dock, ctx);
                                app.show_context_menu = false;
                            }
                        }
                    });

                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

                    // 3. 矢量菜单项列表
                    let pin_txt = if app.flyout_pinned {
                        lang.tr("取消固定并收起遥测面板", "Unpin & Collapse Flyout")
                    } else {
                        lang.tr("展开并固定实时遥测面板", "Expand & Pin Telemetry Flyout")
                    };
                    if menu_row(ui, if app.flyout_pinned { Icon::PinFilled } else { Icon::Pin }, pin_txt, t.accent, t).clicked() {
                        app.flyout_pinned = !app.flyout_pinned;
                        app.cost_dropdown_open = app.flyout_pinned;
                        app.show_context_menu = false;
                    }

                    if menu_row(
                        ui,
                        Icon::GhostPass,
                        lang.tr("开启鼠标点击穿透 (托盘可恢复)", "Enable Click-Through (Tray restores)"),
                        t.text_secondary,
                        t,
                    )
                    .clicked()
                    {
                        app.set_click_through(true);
                        app.show_context_menu = false;
                    }

                    if menu_row(ui, Icon::Calendar, lang.tr("打开日历与趋势统计工作台", "Open Analytics & Calendar Studio"), t.accent, t).clicked() {
                        app.show_settings_window = true;
                        app.settings_tab = 0;
                        app.calendar_state.refresh(&app.collector, &app.config);
                        app.show_context_menu = false;
                    }

                    if menu_row(ui, Icon::Sliders, lang.tr("打开偏好与模型单价设置", "Open Preferences & Pricing"), t.text_primary, t).clicked() {
                        app.show_settings_window = true;
                        app.settings_tab = 3;
                        app.show_context_menu = false;
                    }

                    if menu_row(ui, Icon::Minimize, lang.tr("收起到右下角系统托盘", "Minimize to System Tray"), t.text_secondary, t).clicked() {
                        app.show_context_menu = false;
                        app.minimize_to_tray();
                    }

                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));

                    if menu_row(ui, Icon::Close, lang.tr("关闭退出 AI Token HUD", "Quit AI Token HUD"), t.danger, t).clicked() {
                        app.show_context_menu = false;
                        app.quit_gracefully();
                    }
                });
        });

    if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary) || i.pointer.button_clicked(egui::PointerButton::Secondary)) {
        if let Some(mouse_pos) = ctx.input(|i| i.pointer.latest_pos()) {
            if !menu_response.response.rect.contains(mouse_pos) {
                app.show_context_menu = false;
            }
        }
    }
}

fn menu_row(ui: &mut egui::Ui, icon: Icon, label: &str, fg: Color32, t: &ThemeTokens) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, Rounding::same(t.control_rounding), t.row_hover);
    }
    let ir = Rect::from_center_size(Pos2::new(rect.min.x + 11.0, rect.center().y), Vec2::splat(13.0));
    icons::draw(ui.painter(), ir, icon, fg);
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::prop(11.5), fg);
    ui.painter().galley(Pos2::new(ir.max.x + 7.0, rect.center().y - galley.size().y * 0.5), galley, fg);
    resp
}
