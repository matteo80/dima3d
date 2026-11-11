//! Aspetto dell'interfaccia: temi chiaro e scuro, font, icone.
//!
//! I colori dell'interfaccia stanno tutti qui. La vista 3D ha il proprio sfondo, scelto
//! dall'utente, e non segue il tema: chi confronta le matematiche lo tiene fisso.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

use crate::settings::ThemeMode;

/// Icone uniformi (font Phosphor).
pub use egui_phosphor::regular as icon;

const ICON_SIZE: f32 = 17.0;

/// Font dell'interfaccia: Segoe UI se presente, con le icone e i font di egui come ripiego.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(segoe) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(segoe).into());
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "segoe".into());
        }
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

struct Palette {
    panel: Color32,
    window: Color32,
    field: Color32,
    faint: Color32,
    border: Color32,
    text: Color32,
    text_strong: Color32,
    idle: Color32,
    hover: Color32,
    active: Color32,
    selection: Color32,
    accent: Color32,
    warning: Color32,
    error: Color32,
}

const DARK: Palette = Palette {
    panel: Color32::from_rgb(0x24, 0x27, 0x2C),
    window: Color32::from_rgb(0x2C, 0x30, 0x36),
    field: Color32::from_rgb(0x1B, 0x1D, 0x21),
    faint: Color32::from_rgb(0x2A, 0x2D, 0x33),
    border: Color32::from_rgb(0x3D, 0x42, 0x4A),
    text: Color32::from_rgb(0xD6, 0xDA, 0xE0),
    text_strong: Color32::from_rgb(0xF4, 0xF6, 0xF8),
    idle: Color32::from_rgb(0x33, 0x37, 0x3E),
    hover: Color32::from_rgb(0x3C, 0x41, 0x49),
    active: Color32::from_rgb(0x48, 0x4E, 0x57),
    selection: Color32::from_rgb(0x2B, 0x5C, 0x9A),
    accent: Color32::from_rgb(0x5B, 0xA6, 0xF5),
    warning: Color32::from_rgb(0xE8, 0xB0, 0x4A),
    error: Color32::from_rgb(0xF0, 0x72, 0x6A),
};

const LIGHT: Palette = Palette {
    panel: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    window: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    field: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    faint: Color32::from_rgb(0xEA, 0xEC, 0xEF),
    border: Color32::from_rgb(0xCD, 0xD1, 0xD7),
    text: Color32::from_rgb(0x2A, 0x2E, 0x34),
    text_strong: Color32::from_rgb(0x10, 0x12, 0x16),
    idle: Color32::from_rgb(0xE7, 0xE9, 0xEC),
    hover: Color32::from_rgb(0xDD, 0xE0, 0xE5),
    active: Color32::from_rgb(0xCF, 0xD3, 0xD9),
    selection: Color32::from_rgb(0xBF, 0xDB, 0xFC),
    accent: Color32::from_rgb(0x1F, 0x63, 0xC4),
    warning: Color32::from_rgb(0x8F, 0x52, 0x00),
    error: Color32::from_rgb(0xC6, 0x2F, 0x28),
};

fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

pub fn warning_color(ui: &egui::Ui) -> Color32 {
    palette(ui).warning
}

pub fn error_color(ui: &egui::Ui) -> Color32 {
    palette(ui).error
}

pub fn accent_color(ui: &egui::Ui) -> Color32 {
    palette(ui).accent
}

fn visuals(p: &Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let radius = egui::CornerRadius::same(4);

    v.override_text_color = None;
    v.panel_fill = p.panel;
    v.window_fill = p.window;
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.faint;
    v.code_bg_color = p.faint;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0_f32, p.text_strong);
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, text) in [
        (&mut w.inactive, p.idle, p.text),
        (&mut w.hovered, p.hover, p.text_strong),
        (&mut w.active, p.active, p.text_strong),
        (&mut w.open, p.hover, p.text_strong),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0_f32, text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    v
}

fn style_spacing(style: &mut egui::Style) {
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(6.0, 4.0);
    s.button_padding = egui::vec2(7.0, 3.0);
    s.interact_size.y = 22.0;
    s.menu_margin = egui::Margin::same(6);
    s.window_margin = egui::Margin::same(10);
    // Barre sempre visibili e ben distinte dallo sfondo: cursore nel colore del testo (chiaro sul
    // tema scuro, scuro sul chiaro), guida appena visibile; più marcate sotto il mouse
    s.scroll = egui::style::ScrollStyle {
        floating: false,
        bar_width: 10.0,
        handle_min_length: 24.0,
        bar_inner_margin: 3.0,
        bar_outer_margin: 1.0,
        foreground_color: true,
        dormant_background_opacity: 0.5,
        active_background_opacity: 0.7,
        interact_background_opacity: 0.9,
        dormant_handle_opacity: 0.45,
        active_handle_opacity: 0.65,
        interact_handle_opacity: 0.9,
        ..egui::style::ScrollStyle::solid()
    };

    let proportional = |size| FontId::new(size, FontFamily::Proportional);
    style.text_styles.insert(TextStyle::Heading, proportional(16.0));
    style.text_styles.insert(TextStyle::Body, proportional(13.5));
    style.text_styles.insert(TextStyle::Button, proportional(13.5));
    style.text_styles.insert(TextStyle::Small, proportional(11.5));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace));
}

/// Applica il tema scelto all'interfaccia. Ha effetto dal frame successivo.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, false));
    ctx.all_styles_mut(style_spacing);
    ctx.set_theme(match mode {
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
    });
}

/// Testo con icona: l'icona è più grande del testo, ed entrambi prendono il colore del widget.
pub fn icon_text(ui: &egui::Ui, icon: &str, label: &str) -> egui::WidgetText {
    let format = |font_id| TextFormat {
        font_id,
        color: Color32::PLACEHOLDER,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.append(icon, 0.0, format(FontId::proportional(ICON_SIZE)));
    if !label.is_empty() {
        job.append(label, 6.0, format(TextStyle::Button.resolve(ui.style())));
    }
    job.into()
}

/// Rende piatti i pulsanti di questa `Ui`: lo sfondo compare solo al passaggio del mouse.
pub fn flat_buttons(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_fill = Color32::TRANSPARENT;
}

/// Riquadro sospeso sopra la vista 3D, leggibile su qualsiasi sfondo.
pub fn overlay_frame(ui: &egui::Ui) -> egui::Frame {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.panel.gamma_multiply(0.92))
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(6)
        .inner_margin(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ogni simbolo scritto nell'interfaccia deve esistere nei font distribuiti con il programma,
    /// altrimenti compare come quadratino (Segoe UI, se c'è, si aggiunge solo come prima scelta).
    #[test]
    fn ui_text_has_no_missing_glyphs() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
        let _ = ctx.run(Default::default(), |_| {});

        let sources = [
            include_str!("mod.rs"),
            include_str!("toolbar.rs"),
            include_str!("viewport.rs"),
            include_str!("scene_tree.rs"),
            include_str!("progress.rs"),
            include_str!("color_menu.rs"),
            include_str!("../project.rs"),
            include_str!("../app.rs"),
        ];
        let mut used: Vec<char> = sources
            .iter()
            .flat_map(|s| s.lines())
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| l.split(" // ").next().unwrap_or(l))
            .flat_map(str::chars)
            .filter(|c| !c.is_ascii())
            .collect();
        used.extend([icon::CUBE, icon::EYE, icon::WARNING, icon::SUN, icon::MOON].iter().flat_map(|s| s.chars()));
        used.sort_unstable();
        used.dedup();

        let font = FontId::proportional(14.0);
        let missing: Vec<char> = ctx.fonts(|f| {
            used.into_iter().filter(|c| !f.has_glyphs(&font, &c.to_string())).collect()
        });
        assert!(missing.is_empty(), "simboli mancanti nei font: {missing:?}");
    }

    /// Testo leggibile sugli sfondi di entrambi i temi (rapporto di contrasto WCAG AA).
    #[test]
    fn theme_text_contrast_is_readable() {
        fn luminance(c: Color32) -> f32 {
            let channel = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        for p in [&DARK, &LIGHT] {
            for bg in [p.panel, p.window, p.field, p.idle, p.hover, p.active, p.selection] {
                assert!(contrast(p.text_strong, bg) >= 4.5, "{bg:?}");
            }
            for bg in [p.panel, p.window, p.field, p.idle] {
                assert!(contrast(p.text, bg) >= 4.5, "{bg:?}");
            }
            for fg in [p.warning, p.error, p.accent] {
                assert!(contrast(fg, p.panel) >= 4.5, "{fg:?}");
            }
        }
    }
}
