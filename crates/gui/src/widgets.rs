use std::f32::consts::PI;

use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2,
    pos2, vec2,
};
use snisplit_gui::view::{Mark, Ring};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x16, 0x18, 0x1d);
pub const HERO_ON: Color32 = Color32::from_rgb(0x12, 0x2a, 0x45);
pub const HERO_TROUBLE: Color32 = Color32::from_rgb(0x3a, 0x24, 0x12);
pub const LINE: Color32 = Color32::from_rgb(0x26, 0x2a, 0x32);
pub const ROW: Color32 = Color32::from_rgb(0x1d, 0x20, 0x27);
pub const TEXT: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf6);
pub const SOFT: Color32 = Color32::from_rgb(0xc3, 0xc9, 0xd3);
pub const MUTED: Color32 = Color32::from_rgb(0xb9, 0xc0, 0xcb);
pub const PALE: Color32 = Color32::from_rgb(0xd3, 0xd8, 0xe0);
pub const ACCENT: Color32 = Color32::from_rgb(0x6a, 0xa9, 0xff);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x8c, 0xbd, 0xff);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x16, 0x2e);
pub const RING: Color32 = Color32::from_rgb(0x7d, 0x85, 0x94);
pub const TRACK: Color32 = Color32::from_rgb(0x3a, 0x3f, 0x4a);
pub const WARNING: Color32 = Color32::from_rgb(0xff, 0xb3, 0x66);
pub const BORDER: Color32 = Color32::from_rgb(0x4a, 0x50, 0x5c);
pub const LIGHT: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf6);
pub const ON_LIGHT: Color32 = Color32::from_rgb(0x16, 0x18, 0x1d);
pub const DEEP: Color32 = Color32::from_rgb(0x10, 0x12, 0x16);
pub const TROUBLE_TEXT: Color32 = Color32::from_rgb(0xf6, 0xdc, 0xc4);

const ARC_POINTS: usize = 32;
const ARC_SWEEP: f32 = PI;
const TURNS_PER_SECOND: f32 = 0.8;
const RING_WIDTH_SHARE: f32 = 27.0;
const THINNEST_RING: f32 = 3.0;
const HOVER_GLOW: Color32 = Color32::from_rgba_premultiplied(12, 12, 12, 12);
const SWITCH_SIZE: Vec2 = vec2(40.0, 22.0);
const KNOB_RADIUS: f32 = 8.0;
const KNOB_INSET: f32 = 11.0;
const MARK_SIZE: f32 = 18.0;
const TRIANGLE_SIZE: f32 = 12.0;

pub fn round_button(
    ui: &mut Ui,
    diameter: f32,
    ring: Ring,
    label: &str,
    enabled: bool,
    label_size: f32,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), sense);
    let painter = ui.painter();
    let center = rect.center();
    let width = (diameter / RING_WIDTH_SHARE).max(THINNEST_RING);
    let radius = diameter / 2.0 - width / 2.0;
    let hovered = enabled && response.hovered();
    let mut label_colour = TEXT;
    match ring {
        Ring::Hollow => {
            let colour = match (enabled, hovered) {
                (false, _) => TRACK,
                (true, true) => SOFT,
                (true, false) => RING,
            };
            painter.circle_stroke(center, radius, Stroke::new(width, colour));
            if !enabled {
                label_colour = MUTED;
            }
        }
        Ring::Turning => {
            painter.circle_stroke(center, radius, Stroke::new(width, TRACK));
            let start = ui.input(|input| input.time) as f32 * TURNS_PER_SECOND * 2.0 * PI;
            painter.add(arc(center, radius, start, Stroke::new(width, ACCENT)));
            label_colour = MUTED;
        }
        Ring::Filled => {
            let fill = if hovered { ACCENT_HOVER } else { ACCENT };
            painter.circle_filled(center, diameter / 2.0, fill);
            label_colour = ON_ACCENT;
        }
        Ring::Warning => {
            let thin = width / 2.0;
            painter.circle_stroke(
                center,
                diameter / 2.0 - thin / 2.0,
                Stroke::new(thin, WARNING),
            );
            painter.circle_stroke(
                center,
                diameter / 2.0 - thin * 2.5,
                Stroke::new(thin, WARNING),
            );
        }
    }
    if hovered && ring != Ring::Filled {
        painter.circle_filled(center, radius - width, HOVER_GLOW);
    }
    painter.text(
        center,
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(label_size),
        label_colour,
    );
    response
}

pub fn small_ring(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let width = 2.0;
    let radius = size / 2.0 - width / 2.0;
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), radius, Stroke::new(width, TRACK));
    let start = ui.input(|input| input.time) as f32 * TURNS_PER_SECOND * 2.0 * PI;
    painter.add(arc(
        rect.center(),
        radius,
        start,
        Stroke::new(width, ACCENT),
    ));
}

fn arc(center: Pos2, radius: f32, start: f32, stroke: Stroke) -> Shape {
    let points = (0..=ARC_POINTS)
        .map(|step| {
            let angle = start + ARC_SWEEP * step as f32 / ARC_POINTS as f32;
            center + Vec2::angled(angle) * radius
        })
        .collect();
    Shape::line(points, stroke)
}

pub fn switch(ui: &mut Ui, on: bool, busy: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(SWITCH_SIZE, sense);
    let (track, knob, knob_x) = match (busy, on) {
        (true, _) => (TRACK, MUTED, rect.center().x),
        (false, true) => (ACCENT, ON_ACCENT, rect.right() - KNOB_INSET),
        (false, false) => (TRACK, PALE, rect.left() + KNOB_INSET),
    };
    let track = if enabled && response.hovered() && !on {
        BORDER
    } else {
        track
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(11), track);
    painter.circle_filled(pos2(knob_x, rect.center().y), KNOB_RADIUS, knob);
    response
}

pub fn disclosure(ui: &mut Ui, open: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TRIANGLE_SIZE), Sense::click());
    let c = rect.center();
    let points = if open {
        vec![
            c + vec2(-5.0, -3.0),
            c + vec2(5.0, -3.0),
            c + vec2(0.0, 3.0),
        ]
    } else {
        vec![
            c + vec2(-3.0, -5.0),
            c + vec2(3.0, 0.0),
            c + vec2(-3.0, 5.0),
        ]
    };
    ui.painter()
        .add(Shape::convex_polygon(points, MUTED, Stroke::NONE));
    response
}

pub fn plus(painter: &Painter, center: Pos2, colour: Color32) {
    let stroke = Stroke::new(2.0, colour);
    painter.line_segment([center + vec2(-5.0, 0.0), center + vec2(5.0, 0.0)], stroke);
    painter.line_segment([center + vec2(0.0, -5.0), center + vec2(0.0, 5.0)], stroke);
}

pub fn mark(ui: &mut Ui, mark: Mark) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(MARK_SIZE), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    painter.circle_stroke(c, MARK_SIZE / 2.0 - 0.5, Stroke::new(1.0, PALE));
    let stroke = Stroke::new(1.5, TEXT);
    match mark {
        Mark::Opens => plus_small(painter, c, stroke),
        Mark::Same => {
            painter.line_segment([c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0)], stroke);
            painter.line_segment([c + vec2(-4.0, 2.0), c + vec2(4.0, 2.0)], stroke);
        }
        Mark::Cannot => {
            painter.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], stroke);
            painter.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], stroke);
        }
        Mark::Trouble => {
            painter.line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, 1.0)], stroke);
            painter.circle_filled(c + vec2(0.0, 4.0), 1.0, TEXT);
        }
    }
}

fn plus_small(painter: &Painter, c: Pos2, stroke: Stroke) {
    painter.line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 0.0)], stroke);
    painter.line_segment([c + vec2(0.0, -4.0), c + vec2(0.0, 4.0)], stroke);
}
