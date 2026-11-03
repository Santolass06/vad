//! Visual theme and accessibility rules (PLANO_VAD.md §4.34, Sprint_15).
//!
//! The mockup file `design/vad-ui-design.html` ("Cinema Violet") is not in the repository, so
//! this palette is **not** derived from it: the source of truth is the accent and panel colors
//! the app already used since Sprint_02. Every pair below is checked by a unit test with the
//! WCAG relative-luminance formula, after compositing translucent colors over their real
//! backdrop.

use eframe::egui::{self, Color32, Stroke};

/// Violet accent already used across the HUD and tabs.
pub const ACCENT: Color32 = Color32::from_rgb(139, 124, 246);
/// Side panel fill (translucent: the video can show through).
pub const PANEL_FILL: Color32 = Color32::from_rgba_premultiplied(19, 21, 29, 248);
/// Card inside panels.
pub const CARD_FILL: Color32 = Color32::from_rgb(25, 27, 36);
/// Secondary text and tab labels.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(150, 155, 175);
/// Muted status text (also replaces the ad-hoc greys that fell below 4.5:1).
pub const TEXT_MUTED: Color32 = Color32::from_rgb(160, 165, 185);
pub const OK: Color32 = Color32::from_rgb(74, 222, 128);
pub const WARN: Color32 = Color32::from_rgb(250, 204, 21);
/// Errors (replaces `Color32::RED`, 4.25:1 on the lightest backdrop).
pub const ERROR: Color32 = Color32::from_rgb(248, 113, 113);

/// No text below this size, in points (WCAG-minded floor from §4.34).
pub const MIN_TEXT_SIZE: f32 = 12.0;
/// Width of the keyboard focus ring.
pub const FOCUS_RING_WIDTH: f32 = 2.0;

/// Applies the app-wide visuals. In egui 0.36 a widget with keyboard focus is drawn with the
/// `widgets.active` visuals (`Style::interact`), and a focused text field with
/// `selection.stroke`; both get a 2 px accent outline so focus is always visible.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    let ring = Stroke::new(FOCUS_RING_WIDTH, ACCENT);
    visuals.widgets.active.bg_stroke = ring;
    visuals.selection.stroke = ring;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(139, 124, 246, 90);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
}

// ---------------------------------------------------------------------------------------
// WCAG 2.x contrast
// ---------------------------------------------------------------------------------------

#[cfg(test)]
fn channel(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// WCAG relative luminance of an opaque color.
#[cfg(test)]
pub fn luminance(c: Color32) -> f64 {
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// Alpha-composites straight-alpha `(r,g,b,a)` over an opaque backdrop.
#[cfg(test)]
pub fn over(rgba: [u8; 4], backdrop: Color32) -> Color32 {
    let a = f64::from(rgba[3]) / 255.0;
    let mix = |f: u8, b: u8| (f64::from(f) * a + f64::from(b) * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(rgba[0], backdrop.r()), mix(rgba[1], backdrop.g()), mix(rgba[2], backdrop.b()))
}

/// WCAG contrast ratio between two opaque colors (1.0 – 21.0).
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AA_TEXT: f64 = 4.5;
    const AA_GRAPHIC: f64 = 3.0;

    /// The side panel is translucent (alpha 248) over whatever the video shows: worst cases
    /// are pure black and pure white.
    fn panel_backdrops() -> [Color32; 2] {
        let fill = [20, 22, 30, 248];
        [over(fill, Color32::BLACK), over(fill, Color32::WHITE)]
    }

    #[test]
    fn contrast_formula_matches_the_wcag_reference_values() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-9);
        // #767676 on white is the textbook "just passes AA" (4.54:1).
        assert!((contrast(Color32::from_rgb(0x76, 0x76, 0x76), Color32::WHITE) - 4.54).abs() < 0.02);
    }

    #[test]
    fn panel_const_matches_the_straight_alpha_fill_used_by_the_side_panel() {
        // `from_rgba_unmultiplied(20,22,30,248)` premultiplies to ~(19,21,29,248).
        let un = Color32::from_rgba_unmultiplied(20, 22, 30, 248);
        for (a, b) in [(un.r(), PANEL_FILL.r()), (un.g(), PANEL_FILL.g()), (un.b(), PANEL_FILL.b())] {
            assert!(a.abs_diff(b) <= 1);
        }
    }

    #[test]
    fn text_colors_meet_aa_on_every_backdrop_they_sit_on() {
        let mut backdrops = panel_backdrops().to_vec();
        backdrops.push(CARD_FILL);
        for (name, fg) in [
            ("secondary", TEXT_SECONDARY),
            ("muted", TEXT_MUTED),
            ("accent", ACCENT),
            ("ok", OK),
            ("warn", WARN),
            ("error", ERROR),
        ] {
            for bg in &backdrops {
                let c = contrast(fg, *bg);
                assert!(c >= AA_TEXT, "{name} {fg:?} on {bg:?}: {c:.2}:1 < 4.5");
            }
        }
    }

    /// Regression: `Color32::RED` (pure red) really did fail AA on cards and on the light
    /// backdrop, so `ERROR` replaces it. (`Color32::GRAY` is 160 grey in egui and passes; it was
    /// swapped for `TEXT_MUTED` only for consistency, not because it failed.)
    #[test]
    fn pure_red_really_did_fail_so_the_replacement_matters() {
        assert!(contrast(Color32::RED, CARD_FILL) < AA_TEXT);
        assert!(contrast(Color32::RED, panel_backdrops()[1]) < AA_TEXT);
        assert!(contrast(Color32::GRAY, panel_backdrops()[1]) >= AA_TEXT);
    }

    #[test]
    fn privacy_badges_are_readable_on_their_translucent_backgrounds() {
        for backdrop in panel_backdrops() {
            let local_bg = over([34, 197, 94, 40], backdrop);
            let cloud_bg = over([234, 179, 8, 40], backdrop);
            assert!(contrast(OK, local_bg) >= AA_TEXT, "local badge on {backdrop:?}");
            assert!(contrast(WARN, cloud_bg) >= AA_TEXT, "cloud badge on {backdrop:?}");
        }
    }

    #[test]
    fn focus_ring_and_thin_markers_meet_the_graphic_threshold() {
        let mut backdrops = panel_backdrops().to_vec();
        backdrops.push(CARD_FILL);
        for bg in backdrops {
            assert!(contrast(ACCENT, bg) >= AA_GRAPHIC, "focus ring on {bg:?}");
        }
    }

    #[test]
    fn apply_sets_a_two_pixel_accent_focus_ring() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let v = ctx.global_style().visuals.clone();
        assert_eq!(v.widgets.active.bg_stroke, Stroke::new(2.0, ACCENT));
        assert_eq!(v.selection.stroke, Stroke::new(2.0, ACCENT));
    }

    /// Dark label drawn on an accent-colored button: checked against the accent, not the panel.
    const ON_ACCENT_TEXT: Color32 = Color32::from_rgb(20, 20, 28);

    #[test]
    fn dark_text_on_accent_buttons_meets_aa() {
        assert!(contrast(ON_ACCENT_TEXT, ACCENT) >= AA_TEXT);
    }

    /// Every literal `Color32::from_rgb(..)` used as *text* **on the same source line** as
    /// `colored_label`/`.color(`/`painter.text`/`RichText` (not the named theme constants, tested
    /// above) must meet AA on the worst panel backdrop. Colors passed on a different line of a
    /// multi-line call are NOT seen by this scan.
    #[test]
    fn hard_coded_text_colors_in_sources_meet_aa() {
        fn scan(dir: &std::path::Path, out: &mut Vec<String>) {
            let mut backdrops = panel_backdrops().to_vec();
            backdrops.push(CARD_FILL);
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    scan(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("theme.rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let code = src.split("#[cfg(test)]").next().unwrap_or(&src);
                    for (n, line) in code.lines().enumerate() {
                        let is_text = ["colored_label", ".color(", "painter.text", "RichText"].iter().any(|k| line.contains(k));
                        if !is_text {
                            continue;
                        }
                        let mut rest = line;
                        while let Some(i) = rest.find("Color32::from_rgb(") {
                            rest = &rest[i + "Color32::from_rgb(".len()..];
                            let Some(end) = rest.find(')') else { break };
                            let n3: Vec<u8> = rest[..end].split(',').filter_map(|v| v.trim().parse().ok()).collect();
                            if let [r, g, b] = n3[..] {
                                let c = Color32::from_rgb(r, g, b);
                                if c == ON_ACCENT_TEXT {
                                    continue;
                                }
                                for bg in &backdrops {
                                    let ratio = contrast(c, *bg);
                                    if ratio < AA_TEXT {
                                        out.push(format!("{}:{}: {c:?} on {bg:?} = {ratio:.2}:1", p.display(), n + 1));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut offenders = Vec::new();
        scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut offenders);
        assert!(offenders.is_empty(), "text below AA:\n{}", offenders.join("\n"));
    }

    /// The 12 px floor, enforced over the real sources: no literal font size below it.
    #[test]
    fn no_text_is_smaller_than_the_minimum_size() {
        fn scan(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    scan(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    // Only the code above the test module of each file.
                    let code = src.split("#[cfg(test)]").next().unwrap_or(&src);
                    for (n, line) in code.lines().enumerate() {
                        for pat in [".size(", "FontId::proportional(", "FontId::monospace("] {
                            let mut rest = line;
                            while let Some(i) = rest.find(pat) {
                                rest = &rest[i + pat.len()..];
                                let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                                if let Ok(v) = num.parse::<f32>() {
                                    if v < MIN_TEXT_SIZE && !line.contains("theme: allow-small") {
                                        out.push(format!("{}:{}: {pat}{v})", p.display(), n + 1));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut offenders = Vec::new();
        scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut offenders);
        assert!(offenders.is_empty(), "text below {MIN_TEXT_SIZE}:\n{}", offenders.join("\n"));
    }
}
