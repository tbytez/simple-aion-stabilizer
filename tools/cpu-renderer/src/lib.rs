//! CPU rasterization of egui meshes to an opaque, top-down BGRX bitmap.
//! Keeps egui's tessellated AA fringes and font atlas; no graphics device.
use egui::{
    epaint::{Primitive, Vertex},
    Color32, ColorImage, ImageData, Rect, TextureFilter, TextureId, TextureOptions,
    TextureWrapMode, TexturesDelta,
};
use std::collections::HashMap;

const MAX_SIDE: usize = 16384;
const MAX_PIXELS: usize = 32 * 1024 * 1024;
#[derive(Clone)]
struct Texture {
    image: ColorImage,
    options: TextureOptions,
}
#[derive(Default)]
pub struct Rasterizer {
    textures: HashMap<TextureId, Texture>,
    pixels: Vec<u32>,
    size: [usize; 2],
}
impl Rasterizer {
    pub fn size(&self) -> [usize; 2] {
        self.size
    }
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }
    pub fn update(&mut self, delta: &mut TexturesDelta) -> Result<(), String> {
        for (id, updates) in delta.set.drain() {
            for d in updates {
                let ImageData::Color(image) = &d.image;
                let [w, h] = image.size;
                if w == 0
                    || h == 0
                    || w > MAX_SIDE
                    || h > MAX_SIDE
                    || w.checked_mul(h) != Some(image.pixels.len())
                    || image.pixels.len() > MAX_PIXELS
                {
                    return Err("Invalid or oversized CPU texture".into());
                }
                if let Some([x, y]) = d.pos {
                    let t = self
                        .textures
                        .get_mut(&id)
                        .ok_or("Partial update without texture")?;
                    if x.checked_add(w).is_none_or(|r| r > t.image.size[0])
                        || y.checked_add(h).is_none_or(|b| b > t.image.size[1])
                    {
                        return Err("Partial texture update out of bounds".into());
                    }
                    for row in 0..h {
                        let offset = (y + row) * t.image.size[0] + x;
                        t.image.pixels[offset..offset + w]
                            .copy_from_slice(&image.pixels[row * w..(row + 1) * w]);
                    }
                    t.options = d.options;
                } else {
                    self.textures.insert(
                        id,
                        Texture {
                            image: image.as_ref().clone(),
                            options: d.options,
                        },
                    );
                }
            }
        }
        Ok(())
    }
    pub fn free(&mut self, delta: &mut TexturesDelta) {
        for id in delta.free.drain() {
            self.textures.remove(&id);
        }
    }
    pub fn render(
        &mut self,
        size: [usize; 2],
        scale: f32,
        clear: Color32,
        primitives: &[egui::ClippedPrimitive],
    ) -> Result<(), String> {
        let [w, h] = size;
        if !scale.is_finite()
            || scale <= 0.0
            || w > MAX_SIDE
            || h > MAX_SIDE
            || w.checked_mul(h).is_none_or(|n| n > MAX_PIXELS)
        {
            return Err("CPU render dimensions exceed safe limits".into());
        }
        self.size = size;
        let len = w * h;
        if len > self.pixels.len() {
            self.pixels
                .try_reserve(len - self.pixels.len())
                .map_err(|e| e.to_string())?;
        }
        self.pixels.resize(len, 0);
        self.pixels.fill(pack(clear.to_array()));
        if w == 0 || h == 0 {
            return Ok(());
        }
        for cp in primitives {
            let Primitive::Mesh(mesh) = &cp.primitive else {
                return Err("GPU paint callbacks are unsupported by CPU prototype".into());
            };
            if !mesh.is_valid() {
                return Err("Invalid egui mesh".into());
            }
            let texture = self
                .textures
                .get(&mesh.texture_id)
                .ok_or("Missing CPU texture")?;
            let clip = scissor(cp.clip_rect, scale, size);
            for tri in mesh.indices.as_chunks::<3>().0 {
                triangle(
                    &mut self.pixels,
                    w,
                    clip,
                    scale,
                    [
                        mesh.vertices[tri[0] as usize],
                        mesh.vertices[tri[1] as usize],
                        mesh.vertices[tri[2] as usize],
                    ],
                    texture,
                );
            }
        }
        Ok(())
    }
    pub fn image(&self) -> ColorImage {
        ColorImage::new(
            self.size,
            self.pixels
                .iter()
                .map(|p| Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, *p as u8))
                .collect(),
        )
    }
}
fn pack(c: [u8; 4]) -> u32 {
    (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
}
fn scissor(r: Rect, s: f32, [w, h]: [usize; 2]) -> [i64; 4] {
    [
        (r.min.x * s).round().clamp(0., w as f32) as i64,
        (r.min.y * s).round().clamp(0., h as f32) as i64,
        (r.max.x * s).round().clamp(0., w as f32) as i64,
        (r.max.y * s).round().clamp(0., h as f32) as i64,
    ]
}
fn edge(a: [i64; 2], b: [i64; 2], p: [i64; 2]) -> i64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
fn top_left(a: [i64; 2], b: [i64; 2]) -> bool {
    b[1] < a[1] || (b[1] == a[1] && b[0] > a[0])
}
fn ceil_div(a: i64, b: i64) -> i64 {
    -(-a).div_euclid(b)
}
fn triangle(
    pixels: &mut [u32],
    width: usize,
    clip: [i64; 4],
    scale: f32,
    mut v: [Vertex; 3],
    t: &Texture,
) {
    if v.iter().any(|v| {
        !v.pos.is_finite()
            || !v.uv.is_finite()
            || v.pos.x.abs() * scale > 1_000_000.
            || v.pos.y.abs() * scale > 1_000_000.
    }) {
        return;
    }
    let mut p = v.map(|v| {
        [
            (v.pos.x * scale * 256.).round() as i64,
            (v.pos.y * scale * 256.).round() as i64,
        ]
    });
    let mut area = edge(p[0], p[1], p[2]);
    if area == 0 {
        return;
    }
    if area < 0 {
        p.swap(1, 2);
        v.swap(1, 2);
        area = -area;
    }
    let xmin = clip[0].max(ceil_div(p.iter().map(|v| v[0]).min().unwrap() - 128, 256));
    let xmax = clip[2].min((p.iter().map(|v| v[0]).max().unwrap() - 128).div_euclid(256) + 1);
    let ymin = clip[1].max(ceil_div(p.iter().map(|v| v[1]).min().unwrap() - 128, 256));
    let ymax = clip[3].min((p.iter().map(|v| v[1]).max().unwrap() - 128).div_euclid(256) + 1);
    if xmin >= xmax || ymin >= ymax {
        return;
    }
    let edges = [(p[1], p[2]), (p[2], p[0]), (p[0], p[1])];
    let dx = edges.map(|(a, b)| -(b[1] - a[1]) * 256);
    let limit = edges.map(|(a, b)| if top_left(a, b) { 0 } else { 1 });
    let colors = v.map(|v| v.color.to_array().map(f32::from));
    let same_uv = v[0].uv == v[1].uv && v[0].uv == v[2].uv;
    let same_color = v[0].color == v[1].color && v[0].color == v[2].color;
    let filter = if same_uv {
        t.options.magnification
    } else {
        // Texture-space footprint in physical pixels selects min/mag filtering.
        let a = area as f32;
        let du_dx =
            (v[0].uv.x * dx[0] as f32 + v[1].uv.x * dx[1] as f32 + v[2].uv.x * dx[2] as f32) / a
                * t.image.size[0] as f32;
        let dv_dx =
            (v[0].uv.y * dx[0] as f32 + v[1].uv.y * dx[1] as f32 + v[2].uv.y * dx[2] as f32) / a
                * t.image.size[1] as f32;
        let dy = edges.map(|(a, b)| (b[0] - a[0]) * 256);
        let du_dy =
            (v[0].uv.x * dy[0] as f32 + v[1].uv.x * dy[1] as f32 + v[2].uv.x * dy[2] as f32) / a
                * t.image.size[0] as f32;
        let dv_dy =
            (v[0].uv.y * dy[0] as f32 + v[1].uv.y * dy[1] as f32 + v[2].uv.y * dy[2] as f32) / a
                * t.image.size[1] as f32;
        if du_dx.hypot(dv_dx).max(du_dy.hypot(dv_dy)) > 1. {
            t.options.minification
        } else {
            t.options.magnification
        }
    };
    let solid = if same_uv && same_color {
        Some(modulate(colors[0], sample(t, v[0].uv.x, v[0].uv.y, filter)))
    } else {
        None
    };
    let inv = 1.0 / area as f32;
    for y in ymin..ymax {
        let base = edges.map(|(a, b)| edge(a, b, [xmin * 256 + 128, y * 256 + 128]));
        let mut lo = 0;
        let mut hi = xmax - xmin - 1;
        for k in 0..3 {
            if dx[k] > 0 {
                lo = lo.max(ceil_div(limit[k] - base[k], dx[k]));
            } else if dx[k] < 0 {
                hi = hi.min((base[k] - limit[k]).div_euclid(-dx[k]));
            } else if base[k] < limit[k] {
                hi = -1;
            }
        }
        if lo > hi {
            continue;
        }
        let from = y as usize * width + (xmin + lo) as usize;
        let row = &mut pixels[from..from + (hi - lo + 1) as usize];
        if let Some(c) = solid {
            if c[3] >= 254.999 {
                row.fill(pack(c.map(|c| c.clamp(0., 255.).round() as u8)));
            } else {
                for dst in row {
                    blend(dst, c);
                }
            }
            continue;
        }
        for (j, dst) in row.iter_mut().enumerate() {
            let x = lo + j as i64;
            let b = [
                (base[0] + dx[0] * x) as f32 * inv,
                (base[1] + dx[1] * x) as f32 * inv,
                (base[2] + dx[2] * x) as f32 * inv,
            ];
            let color = std::array::from_fn(|k| {
                colors[0][k] * b[0] + colors[1][k] * b[1] + colors[2][k] * b[2]
            });
            let u = v[0].uv.x * b[0] + v[1].uv.x * b[1] + v[2].uv.x * b[2];
            let vv = v[0].uv.y * b[0] + v[1].uv.y * b[1] + v[2].uv.y * b[2];
            blend(dst, modulate(color, sample(t, u, vv, filter)));
        }
    }
}
fn modulate(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|k| a[k] * b[k] / 255.)
}
fn blend(dst: &mut u32, c: [f32; 4]) {
    // egui colors/textures are premultiplied gamma-encoded sRGBA. Additive
    // colors (alpha=0, RGB>0) must not be discarded.
    let inv = 1.0 - c[3].clamp(0., 255.) / 255.;
    let r = (c[0] + ((*dst >> 16) & 255) as f32 * inv)
        .clamp(0., 255.)
        .round() as u32;
    let g = (c[1] + ((*dst >> 8) & 255) as f32 * inv)
        .clamp(0., 255.)
        .round() as u32;
    let b = (c[2] + (*dst & 255) as f32 * inv).clamp(0., 255.).round() as u32;
    *dst = (r << 16) | (g << 8) | b;
}
fn coord(x: i64, n: usize, wrap: TextureWrapMode) -> usize {
    let n = n as i64;
    match wrap {
        TextureWrapMode::ClampToEdge => x.clamp(0, n - 1) as usize,
        TextureWrapMode::Repeat => x.rem_euclid(n) as usize,
        TextureWrapMode::MirroredRepeat => {
            let p = x.rem_euclid(n * 2);
            p.min(n * 2 - 1 - p) as usize
        }
    }
}
fn sample(t: &Texture, u: f32, v: f32, filter: TextureFilter) -> [f32; 4] {
    let [w, h] = t.image.size;
    let bounded = |v: f32| match t.options.wrap_mode {
        TextureWrapMode::ClampToEdge => v.clamp(0., 1.),
        TextureWrapMode::Repeat => v.rem_euclid(1.),
        TextureWrapMode::MirroredRepeat => v.rem_euclid(2.),
    };
    let u = bounded(u);
    let v = bounded(v);
    let texel = |x: i64, y: i64| {
        t.image.pixels[coord(y, h, t.options.wrap_mode) * w + coord(x, w, t.options.wrap_mode)]
            .to_array()
            .map(f32::from)
    };
    if filter == TextureFilter::Nearest {
        return texel((u * w as f32).floor() as i64, (v * h as f32).floor() as i64);
    }
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let a = texel(ix, iy);
    let b = texel(ix + 1, iy);
    let c = texel(ix, iy + 1);
    let d = texel(ix + 1, iy + 1);
    std::array::from_fn(|k| {
        (a[k] * (1. - fx) + b[k] * fx) * (1. - fy) + (c[k] * (1. - fx) + d[k] * fx) * fy
    })
}
#[cfg(windows)]
pub mod gdi;
#[cfg(test)]
mod tests;

pub mod diagnostics;

/// Grow in modest blocks to avoid creating GDI objects for every resize pixel.
/// Retention is bounded by the rasterizer's existing pixel/side limits.
#[cfg(any(windows, test))]
fn dib_capacity(previous: Option<[usize; 2]>, size: [usize; 2]) -> [usize; 2] {
    let old = previous.unwrap_or([0, 0]);
    let result = [
        size[0].max(old[0]).div_ceil(128) * 128,
        size[1].max(old[1]).div_ceil(128) * 128,
    ];
    if result[0] > MAX_SIDE || result[1] > MAX_SIDE || result[0] * result[1] > MAX_PIXELS {
        size
    } else {
        result
    }
}
#[cfg(test)]
mod capacity_tests {
    use super::*;
    #[test]
    fn retained_dib_growth_is_bounded_and_covers_every_requested_row() {
        for size in [
            [1, 1],
            [420, 700],
            [1920, 1080],
            [3840, 2160],
            [16384, 2048],
            [2048, 16384],
        ] {
            for old in [None, Some([4096, 4096]), Some([16384, 2048])] {
                let cap = dib_capacity(old, size);
                assert!(cap[0] >= size[0] && cap[1] >= size[1]);
                assert!(cap[0] <= MAX_SIDE && cap[1] <= MAX_SIDE && cap[0] * cap[1] <= MAX_PIXELS);
            }
        }
        assert_eq!(dib_capacity(Some([512, 768]), [421, 701]), [512, 768]);
    }
}

#[cfg(any(windows, test))]
fn copy_dib_rows(destination: &mut [u32], stride: usize, pixels: &[u32], size: [usize; 2]) {
    let [w, h] = size;
    for y in 0..h {
        destination[y * stride..y * stride + w].copy_from_slice(&pixels[y * w..(y + 1) * w]);
    }
}
#[cfg(test)]
mod dib_copy_tests {
    use super::*;
    #[test]
    fn copying_smaller_frames_preserves_stride_and_row_orientation() {
        let mut destination = vec![0xdeadbeef; 8 * 8];
        let pixels = [1, 2, 3, 4, 5, 6];
        copy_dib_rows(&mut destination, 8, &pixels, [3, 2]);
        assert_eq!(&destination[..3], &[1, 2, 3]);
        assert_eq!(&destination[8..11], &[4, 5, 6]);
        assert!(destination[3..8].iter().all(|p| *p == 0xdeadbeef));
        assert!(destination[11..].iter().all(|p| *p == 0xdeadbeef));
        copy_dib_rows(&mut destination, 8, &[7, 8], [1, 2]);
        assert_eq!(destination[0], 7);
        assert_eq!(destination[8], 8);
    }
}
