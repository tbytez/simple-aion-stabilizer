//! Antialiased vector drawing for the dashboard: the gradient power ring,
//! the header brand mark and a small icon set. Icons share a 24-unit grid,
//! one stroke weight and rounded ends, matching the approved visual language.

use egui::{vec2, Color32, Mesh, Pos2, Rect, Shape, Stroke};
use std::f32::consts::{FRAC_PI_2, TAU};

/// Uniform icon stroke weight, in units of the shared 24x24 icon grid.
const ICON_STROKE: f32 = 1.8;

/// Chord length, in points, used when sampling arcs and curves. Short enough
/// that the sagitta stays far below a physical pixel at any monitor scale.
const CURVE_CHORD: f32 = 1.5;

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}

/// Green used for "running" and success affordances, per theme.
pub fn success(ctx: &egui::Context) -> Color32 {
    if ctx.theme() == egui::Theme::Light {
        Color32::from_rgb(4, 120, 87)
    } else {
        Color32::from_rgb(52, 211, 153)
    }
}

/// egui antialiases every built-in shape with a one-physical-pixel alpha
/// fringe (`TessellationOptions::feathering_size_in_pixels`); raw meshes get
/// no such treatment, so the ring emits the same fringe itself. Returns the
/// fringe width in points for the current frame.
pub fn feather(ctx: &egui::Context) -> f32 {
    let px = ctx.tessellation_options(|o| {
        if o.feathering {
            o.feathering_size_in_pixels
        } else {
            0.0
        }
    });
    (px / ctx.pixels_per_point()).max(0.0)
}

/// Chords of at most ~4 points keep the sagitta under a tenth of a point at
/// the sizes used here, with a floor so small rings stay round too.
fn ring_segments(outer_r: f32) -> usize {
    ((outer_r * TAU / 4.0).ceil() as usize).clamp(48, 720)
}

/// Gradient ring mesh with coverage ramps on both edges, mirroring how
/// egui's tessellator antialiases a closed stroke: the fully opaque band
/// ends `feather/2` inside each nominal edge and alpha ramps to zero
/// `feather/2` beyond it. A nominal edge therefore lands at ~50% coverage
/// and the shape grows `feather/2` past it in each direction. The fringe
/// fades to transparent so the edge blends into whatever is underneath
/// (the power disc inside, the card outside).
pub fn ring_mesh(
    center: Pos2,
    inner_r: f32,
    outer_r: f32,
    feather: f32,
    a: Color32,
    b: Color32,
) -> Mesh {
    let width = (outer_r - inner_r).max(0.0);
    let mid = 0.5 * (inner_r + outer_r);
    // A feather wider than half the band would collapse the opaque core.
    let feather = feather.clamp(0.0, width * 0.5);
    let opaque = 0.5 * (width - feather);
    let extent = 0.5 * (width + feather);
    let n = ring_segments(mid + extent);
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(4 * (n + 1));
    mesh.reserve_triangles(6 * n);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let ang = t * TAU - FRAC_PI_2;
        let dir = vec2(ang.cos(), ang.sin());
        let col = lerp(a, b, 0.5 + 0.5 * (t * TAU).cos());
        mesh.colored_vertex(center + dir * (mid + extent), Color32::TRANSPARENT);
        mesh.colored_vertex(center + dir * (mid + opaque), col);
        mesh.colored_vertex(center + dir * (mid - opaque), col);
        mesh.colored_vertex(center + dir * (mid - extent), Color32::TRANSPARENT);
    }
    for i in 0..n {
        let p = (4 * i) as u32;
        let c = (4 * (i + 1)) as u32;
        for k in 0..3 {
            mesh.add_triangle(p + k, p + k + 1, c + k);
            mesh.add_triangle(p + k + 1, c + k + 1, c + k);
        }
    }
    mesh
}

/// Static gradient ring (color `b` at the top through `a` at the bottom),
/// antialiased at every monitor scale. No animation or repaint loop.
pub fn gradient_ring(
    painter: &egui::Painter,
    center: Pos2,
    inner_r: f32,
    outer_r: f32,
    a: Color32,
    b: Color32,
) {
    let feather = feather(painter.ctx());
    painter.add(Shape::mesh(ring_mesh(center, inner_r, outer_r, feather, a, b)));
}

/// The icon set: shared 24-unit grid, one stroke weight, rounded ends.
#[derive(Clone, Copy)]
pub enum Icon {
    Power,
    Game,
    Window,
    Check,
    Update,
    Tray,
}

/// Paint an icon centered in `rect`.
pub fn icon(painter: &egui::Painter, kind: Icon, rect: Rect, color: Color32) {
    let g = Glyph::new(painter, rect, color);
    match kind {
        Icon::Power => {
            // M12 3v9  M7 5.8a8 8 0 1 0 10 0
            g.open(&[g.pt(12.0, 3.0), g.pt(12.0, 12.0)]);
            g.open(&g.arc(12.0, 12.0, 8.0, -128.93, -411.07));
        }
        Icon::Game => {
            // Gamepad outline; curves are sampled from the source beziers.
            let mut pts = vec![g.pt(7.5, 7.0), g.pt(16.5, 7.0)];
            g.cubic(&mut pts, [18.5, 7.0], [19.5, 8.5], [20.0, 11.0]);
            pts.push(g.pt(21.0, 16.0));
            g.cubic(&mut pts, [21.4, 18.0], [19.5, 19.0], [18.0, 17.5]);
            pts.extend([g.pt(15.5, 15.0), g.pt(8.5, 15.0), g.pt(6.0, 17.5)]);
            g.cubic(&mut pts, [4.5, 19.0], [2.6, 18.0], [3.0, 16.0]);
            pts.push(g.pt(4.0, 11.0));
            g.cubic(&mut pts, [4.5, 8.5], [5.5, 7.0], [7.5, 7.0]);
            let corners = [
                g.pt(18.0, 17.5),
                g.pt(15.5, 15.0),
                g.pt(8.5, 15.0),
                g.pt(6.0, 17.5),
            ];
            g.closed(&pts, &corners);
            // d-pad plus and the two action buttons
            g.open(&[g.pt(7.0, 10.0), g.pt(7.0, 14.0)]);
            g.open(&[g.pt(5.0, 12.0), g.pt(9.0, 12.0)]);
            g.dot(g.pt(16.0, 11.0));
            g.dot(g.pt(18.0, 13.0));
        }
        Icon::Window => {
            g.rrect(3.0, 4.0, 18.0, 16.0, 2.0);
            g.stroke(&[g.pt(3.0, 9.0), g.pt(21.0, 9.0)], false);
            g.dot(g.pt(6.0, 6.0));
            g.dot(g.pt(8.0, 6.0));
        }
        Icon::Check => {
            g.circle(12.0, 12.0, 9.0);
            g.open(&[g.pt(8.0, 12.0), g.pt(11.0, 15.0), g.pt(16.0, 9.0)]);
        }
        Icon::Update => {
            // Two arcs chasing each other, each ending in an L arrowhead.
            let mut top = g.arc(12.04, 10.98, 8.5, -20.48, -135.26);
            top.push(g.pt(3.0, 8.0));
            g.open(&top);
            g.open(&[g.pt(3.0, 3.0), g.pt(3.0, 8.0), g.pt(8.0, 8.0)]);
            let mut bot = g.arc(11.96, 13.02, 8.5, 159.48, 44.72);
            bot.push(g.pt(21.0, 16.0));
            g.open(&bot);
            g.open(&[g.pt(21.0, 21.0), g.pt(21.0, 16.0), g.pt(16.0, 16.0)]);
        }
        Icon::Tray => {
            let body = [
                g.pt(4.0, 14.0),
                g.pt(6.0, 6.0),
                g.pt(18.0, 6.0),
                g.pt(20.0, 14.0),
                g.pt(20.0, 19.0),
                g.pt(4.0, 19.0),
            ];
            g.closed(&body, &body);
            let notch = [
                g.pt(4.0, 14.0),
                g.pt(8.0, 14.0),
                g.pt(10.0, 17.0),
                g.pt(14.0, 17.0),
                g.pt(16.0, 14.0),
                g.pt(20.0, 14.0),
            ];
            g.open(&notch);
        }
    }
}

/// Header brand mark: gradient ring with an "A" glyph on a 32-unit grid.
/// Mirrors assets/brand.svg, the single vector source for every identity
/// surface (window, taskbar, exe and tray icons).
pub fn brand_mark(
    painter: &egui::Painter,
    rect: Rect,
    ring_a: Color32,
    ring_b: Color32,
    glyph: Color32,
) {
    let unit = rect.width().min(rect.height()) / 32.0;
    gradient_ring(painter, rect.center(), 10.7 * unit, 13.3 * unit, ring_a, ring_b);
    let g = Glyph {
        painter,
        origin: rect.center() - vec2(16.0, 16.0) * unit,
        unit,
        stroke: Stroke::new(1.9 * unit, glyph),
    };
    g.open(&[g.pt(9.5, 23.0), g.pt(16.0, 9.0), g.pt(22.5, 23.0)]);
    g.open(&[g.pt(12.5, 18.0), g.pt(19.5, 18.0)]);
}

/// One icon being painted: maps the 24-unit grid into a target rect.
struct Glyph<'p> {
    painter: &'p egui::Painter,
    origin: Pos2,
    unit: f32,
    stroke: Stroke,
}

impl<'p> Glyph<'p> {
    fn new(painter: &'p egui::Painter, rect: Rect, color: Color32) -> Self {
        let unit = rect.width().min(rect.height()) / 24.0;
        Self {
            painter,
            origin: rect.center() - vec2(12.0, 12.0) * unit,
            unit,
            stroke: Stroke::new(ICON_STROKE * unit, color),
        }
    }

    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.unit
    }

    /// Round cap / join: a dot of stroke radius centered on the path.
    fn dot(&self, p: Pos2) {
        self.painter
            .circle_filled(p, self.stroke.width * 0.5, self.stroke.color);
    }

    fn stroke(&self, pts: &[Pos2], closed: bool) {
        let shape = if closed {
            Shape::closed_line(pts.to_vec(), self.stroke)
        } else {
            Shape::line(pts.to_vec(), self.stroke)
        };
        self.painter.add(shape);
    }

    /// Open polyline with rounded caps and joins.
    fn open(&self, pts: &[Pos2]) {
        self.stroke(pts, false);
        for &p in pts {
            self.dot(p);
        }
    }

    /// Closed polyline; `corners` also get dots for round joins.
    fn closed(&self, pts: &[Pos2], corners: &[Pos2]) {
        self.stroke(pts, true);
        for &p in corners {
            self.dot(p);
        }
    }

    fn circle(&self, cx: f32, cy: f32, r: f32) {
        self.painter
            .circle_stroke(self.pt(cx, cy), r * self.unit, self.stroke);
    }

    fn rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let rect = Rect::from_min_size(self.pt(x, y), vec2(w, h) * self.unit);
        self.painter.rect_stroke(
            rect,
            r * self.unit,
            self.stroke,
            egui::StrokeKind::Middle,
        );
    }

    /// Arc points from `a0` to `a1` degrees in screen space (y down, 0deg =
    /// +x, increasing angle sweeps clockwise). Sampling is scale-aware.
    fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
        let len = (a1 - a0).abs().to_radians() * r * self.unit;
        let n = (len / CURVE_CHORD).ceil().max(2.0) as usize;
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                self.pt(cx + a.cos() * r, cy + a.sin() * r)
            })
            .collect()
    }

    /// Sample a cubic bezier from the last point of `pts` through the given
    /// control points, appending the result (first point excluded).
    fn cubic(&self, pts: &mut Vec<Pos2>, c1: [f32; 2], c2: [f32; 2], end: [f32; 2]) {
        let (from, c1, c2, end) = (
            pts.last().unwrap().to_vec2(),
            self.pt(c1[0], c1[1]).to_vec2(),
            self.pt(c2[0], c2[1]).to_vec2(),
            self.pt(end[0], end[1]).to_vec2(),
        );
        let len = (c1 - from).length() + (c2 - c1).length() + (end - c2).length();
        let n = (len / CURVE_CHORD).ceil().max(3.0) as usize;
        pts.extend((1..=n).map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (from * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + end * (t * t * t))
                .to_pos2()
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Pos2 = Pos2::new(100.0, 60.0);

    fn radius(p: Pos2) -> f32 {
        (p - C).length()
    }

    #[test]
    fn ring_mesh_has_coverage_fringe_on_both_edges() {
        let (inner, outer, feather) = (42.0, 48.0, 1.0);
        let mesh = ring_mesh(C, inner, outer, feather, Color32::RED, Color32::BLUE);
        assert!(mesh.vertices.len() >= 4 * 49);
        assert_eq!(mesh.vertices.len() % 4, 0);
        let n = mesh.vertices.len() / 4;
        // Three quads (six triangles) per angular segment.
        assert_eq!(mesh.indices.len(), 18 * (n - 1));
        for v in mesh.vertices.as_chunks::<4>().0 {
            let radii: Vec<f32> = v.iter().map(|p| radius(p.pos)).collect();
            assert!((radii[0] - 48.5).abs() < 1e-3, "outer fade edge");
            assert!((radii[1] - 47.5).abs() < 1e-3, "outer opaque edge");
            assert!((radii[2] - 42.5).abs() < 1e-3, "inner opaque edge");
            assert!((radii[3] - 41.5).abs() < 1e-3, "inner fade edge");
            assert_eq!(v[0].color, Color32::TRANSPARENT);
            assert_eq!(v[3].color, Color32::TRANSPARENT);
            assert_eq!(v[1].color.a(), 255);
            assert_eq!(v[2].color.a(), 255);
            // The opaque band shares one gradient color across its width.
            assert_eq!(v[1].color, v[2].color);
        }
        for idx in &mesh.indices {
            assert!((*idx as usize) < mesh.vertices.len());
        }
    }

    #[test]
    fn ring_segments_grow_with_radius_and_stay_bounded() {
        let small = ring_mesh(C, 5.0, 8.0, 0.5, Color32::RED, Color32::BLUE);
        let large = ring_mesh(C, 60.0, 66.0, 0.5, Color32::RED, Color32::BLUE);
        assert!(large.vertices.len() > small.vertices.len());
        // All positions finite and inside the faded band extents.
        for v in large.vertices.iter().chain(small.vertices.iter()) {
            assert!(v.pos.is_finite());
        }
        for v in &large.vertices {
            let r = radius(v.pos);
            assert!((59.5..=66.75).contains(&r), "radius {r}");
        }
    }

    #[test]
    fn ring_mesh_degrades_gracefully_without_feather() {
        let mesh = ring_mesh(C, 42.0, 48.0, 0.0, Color32::RED, Color32::BLUE);
        assert!(!mesh.vertices.is_empty());
        for v in &mesh.vertices {
            let r = radius(v.pos);
            assert!((41.99..=48.01).contains(&r), "radius {r}");
        }
        // An absurd feather is clamped instead of collapsing the band.
        let wide = ring_mesh(C, 42.0, 48.0, 99.0, Color32::RED, Color32::BLUE);
        for v in wide.vertices.as_chunks::<4>().0 {
            assert!(radius(v[1].pos) < radius(v[0].pos));
            assert!(radius(v[2].pos) > radius(v[3].pos));
        }
    }
}
