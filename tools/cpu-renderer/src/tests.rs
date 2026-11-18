struct Deltas {
    set: Vec<(TextureId, ImageDelta)>,
    free: Vec<TextureId>,
}
impl Deltas {
    fn build(self) -> TexturesDelta {
        let mut d = TexturesDelta::default();
        for (id, image) in self.set {
            d.push(id, image);
        }
        for id in self.free {
            d.free(id);
        }
        d
    }
}
use super::*;
use egui::{
    epaint::{ClippedPrimitive, ImageDelta, Mesh},
    pos2,
};
fn white() -> Rasterizer {
    let mut r = Rasterizer::default();
    r.update(
        &mut Deltas {
            set: vec![(
                TextureId::default(),
                ImageDelta::full(
                    ColorImage::filled([1, 1], Color32::WHITE),
                    TextureOptions::LINEAR,
                ),
            )],
            free: vec![],
        }
        .build(),
    )
    .unwrap();
    r
}
fn quad(rect: Rect, color: Color32) -> ClippedPrimitive {
    let mut mesh = Mesh::default();
    mesh.add_colored_rect(rect, color);
    ClippedPrimitive {
        clip_rect: Rect::EVERYTHING,
        primitive: Primitive::Mesh(mesh),
    }
}
#[test]
fn shared_edges_do_not_double_blend_or_leave_cracks_at_any_scale() {
    for scale in [1., 1.25, 1.5, 2., 2.625, 3., 8.] {
        let n = (16. * scale) as usize;
        let mut r = white();
        let q = quad(
            Rect::from_min_max(pos2(0., 0.), pos2(16., 16.)),
            Color32::from_rgba_premultiplied(128, 0, 0, 128),
        );
        r.render([n, n], scale, Color32::BLACK, std::slice::from_ref(&q))
            .unwrap();
        assert!(r.pixels.iter().all(|p| *p == 0x800000), "scale {scale}");
        let mut reverse = q;
        if let Primitive::Mesh(m) = &mut reverse.primitive {
            for t in m.indices.as_chunks_mut::<3>().0 {
                t.swap(0, 2);
            }
        }
        r.render([n, n], scale, Color32::BLACK, &[reverse]).unwrap();
        assert!(r.pixels.iter().all(|p| *p == 0x800000));
    }
}
#[test]
fn clipping_and_scale_use_physical_pixels() {
    let mut r = white();
    let mut q = quad(
        Rect::from_min_max(pos2(-10., -10.), pos2(20., 20.)),
        Color32::RED,
    );
    q.clip_rect = Rect::from_min_max(pos2(2., 3.), pos2(5., 7.));
    r.render([20, 20], 2., Color32::BLACK, &[q]).unwrap();
    for y in 0..20 {
        for x in 0..20 {
            assert_eq!(
                r.pixels[y * 20 + x],
                if (4..10).contains(&x) && (6..14).contains(&y) {
                    0xff0000
                } else {
                    0
                }
            );
        }
    }
}
#[test]
fn texture_updates_filtering_wrapping_and_free() {
    let mut r = white();
    let id = TextureId::default();
    r.update(
        &mut Deltas {
            set: vec![(
                id,
                ImageDelta::full(
                    ColorImage::new([2, 1], vec![Color32::RED, Color32::BLUE]),
                    TextureOptions::LINEAR,
                ),
            )],
            free: vec![],
        }
        .build(),
    )
    .unwrap();
    let t = &r.textures[&id];
    assert_eq!(
        sample(t, 0.5, 0.5, TextureFilter::Linear),
        [127.5, 0., 127.5, 255.]
    );
    assert_eq!(
        sample(t, 0.5, 0.5, TextureFilter::Nearest),
        [0., 0., 255., 255.]
    );
    assert_eq!(
        sample(t, -2., 0.5, TextureFilter::Linear),
        [255., 0., 0., 255.]
    );
    let mut t = t.clone();
    t.options.wrap_mode = TextureWrapMode::Repeat;
    assert_eq!(
        sample(&t, 1.25, 0.5, TextureFilter::Nearest),
        [255., 0., 0., 255.]
    );
    t.options.wrap_mode = TextureWrapMode::MirroredRepeat;
    assert_eq!(
        sample(&t, 1.25, 0.5, TextureFilter::Nearest),
        [0., 0., 255., 255.]
    );
    r.update(
        &mut Deltas {
            set: vec![(
                id,
                ImageDelta::partial(
                    [1, 0],
                    ColorImage::filled([1, 1], Color32::GREEN),
                    TextureOptions::NEAREST,
                ),
            )],
            free: vec![],
        }
        .build(),
    )
    .unwrap();
    assert_eq!(r.textures[&id].image.pixels[1], Color32::GREEN);
    let d = Deltas {
        set: vec![],
        free: vec![id],
    };
    r.free(&mut d.build());
    assert!(r.textures.is_empty());
}
#[test]
fn gradient_and_premultiplied_additive_blending() {
    let mut r = white();
    let m = Mesh { vertices: vec![
        Vertex {
            pos: pos2(0., 0.),
            uv: pos2(0., 0.),
            color: Color32::RED,
        },
        Vertex {
            pos: pos2(4., 0.),
            uv: pos2(0., 0.),
            color: Color32::GREEN,
        },
        Vertex {
            pos: pos2(0., 4.),
            uv: pos2(0., 0.),
            color: Color32::BLUE,
        },
    ], indices: vec![0,1,2], ..Default::default() };
    r.render(
        [4, 4],
        1.,
        Color32::BLACK,
        &[ClippedPrimitive {
            clip_rect: Rect::EVERYTHING,
            primitive: Primitive::Mesh(m),
        }],
    )
    .unwrap();
    assert_eq!(r.pixels[0], 0xbf2020);
    let mut p = 0x101010;
    blend(&mut p, [32., 16., 8., 0.]);
    assert_eq!(p, 0x302018);
}
#[test]
fn invalid_sizes_updates_and_meshes_fail_without_unbounded_allocation() {
    let mut r = white();
    assert!(r.render([usize::MAX, 2], 1., Color32::BLACK, &[]).is_err());
    assert!(r.render([2, 2], f32::NAN, Color32::BLACK, &[]).is_err());
    assert!(r
        .update(
            &mut Deltas {
                set: vec![(
                    TextureId::default(),
                    ImageDelta::partial(
                        [1, 0],
                        ColorImage::filled([1, 1], Color32::RED),
                        TextureOptions::LINEAR
                    )
                )],
                free: vec![]
            }
            .build()
        )
        .is_err());
    let m = Mesh { indices: vec![0,1,2], ..Default::default() };
    assert!(r
        .render(
            [4, 4],
            1.,
            Color32::BLACK,
            &[ClippedPrimitive {
                clip_rect: Rect::EVERYTHING,
                primitive: Primitive::Mesh(m)
            }]
        )
        .is_err());
    r.render([0, 0], 1., Color32::BLACK, &[]).unwrap();
    assert!(r.pixels.is_empty());
}
#[test]
fn degenerate_and_offscreen_triangles_do_not_write() {
    let mut r = white();
    r.render(
        [4, 4],
        1.,
        Color32::BLACK,
        &[
            quad(Rect::from_min_max(pos2(0., 0.), pos2(0., 4.)), Color32::RED),
            quad(
                Rect::from_min_max(pos2(-4., -4.), pos2(-1., -1.)),
                Color32::RED,
            ),
        ],
    )
    .unwrap();
    assert!(r.pixels.iter().all(|p| *p == 0));
}
