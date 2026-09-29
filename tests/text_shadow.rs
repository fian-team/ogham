//! `shadow` on a Text style — CSS `text-shadow`. One map or a list of
//! them, the panel's own `Shadow` shape, painted under the outline and the
//! fill. The pixel test drives a real `SkiaEnv` over an offscreen raster
//! surface, because what a shadow promises is where it lands.

use ogham::runtime::config::RuntimeConfig;
use ogham::skia::SkiaEnv;
use ogham::widget::style::{Color, Shadow, TextStyle};
use ogham::widget::text_widget::TextWidget;
use ogham::widget::vocabulary::scan_source;
use ogham::widget::{RenderContext, WidgetRef};
use ogham::Ogham;

fn find_text(node: &WidgetRef) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.downcast_ref::<TextWidget>().is_some() {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(find_text)
}

fn shadows_of(src: &str) -> Vec<Shadow> {
    let mut o = Ogham::from_source(src, RuntimeConfig::default()).expect("from_source");
    o.frame(400.0, 300.0, 1.0 / 60.0).expect("frame");
    let text = find_text(&o.get_ui().root).expect("a Text in the tree");
    let g = text.lock().unwrap();
    g.downcast_ref::<TextWidget>()
        .unwrap()
        .effective_style()
        .get_shadows()
        .to_vec()
}

#[test]
fn one_shadow_map_is_one_shadow() {
    let got = shadows_of(
        r#"let main = fn () { Text { text: "Galdonni", style: {
            shadow: { color: { r: 0, g: 0, b: 0, a: 180 }, blur: 2, offset_y: 1 },
        } } };"#,
    );
    assert_eq!(
        got,
        [Shadow { color: Color::new(0, 0, 0, 180), blur: 2.0, offset_x: 0.0, offset_y: 1.0 }]
    );
}

/// The tight edge and the wide halo, in the order written — the order they
/// paint in.
#[test]
fn a_list_of_shadows_keeps_its_order() {
    let got = shadows_of(
        r#"let main = fn () { Text { text: "Galdonni", style: {
            shadow: [
              { color: { r: 0, g: 0, b: 0, a: 180 }, blur: 1, offset_y: 1 },
              { color: { r: 0, g: 0, b: 0, a: 115 }, blur: 9 },
            ],
        } } };"#,
    );
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].blur, 1.0);
    assert_eq!(got[1].blur, 9.0);
    assert_eq!(got[1].color.a, 115);
}

#[test]
fn no_shadow_key_is_no_shadow() {
    assert!(shadows_of(r#"let main = fn () { Text { text: "x", style: { size: 12 } } };"#)
        .is_empty());
}

/// The vocabulary knows the key, and knows the map's own keys: a shadow
/// spelled in CSS's camelCase is a report rather than a silent drop.
#[test]
fn the_vocabulary_checks_a_text_shadow() {
    let clean = scan_source(
        "fixture.ogh",
        r#"let main = fn () { Text { text: "x", style: {
            shadow: { color: { r: 0, g: 0, b: 0, a: 1 }, blur: 1, offset_x: 0, offset_y: 1 },
        } } };"#,
    );
    assert!(clean.is_empty(), "{:?}", clean.iter().map(|v| &v.path).collect::<Vec<_>>());
    let wrong: Vec<String> = scan_source(
        "fixture.ogh",
        r#"let main = fn () { Text { text: "x", style: {
            shadow: { color: { r: 0, g: 0, b: 0, a: 1 }, offsetY: 1 },
        } } };"#,
    )
    .into_iter()
    .map(|v| v.path)
    .collect();
    assert_eq!(wrong, ["style.shadow.offsetY"]);
}

fn red_under(env: &mut SkiaEnv, style: &TextStyle) -> usize {
    env.surface.canvas().clear(ogham::skia_safe::Color::WHITE);
    env.draw_text("MMMM", style, 10.0, 10.0, 200.0);
    let info = env.surface.image_info();
    let mut px = vec![0u8; (info.width() * info.height() * 4) as usize];
    assert!(env.surface.read_pixels(
        &info,
        &mut px,
        (info.width() * 4) as usize,
        (0, 0),
    ));
    // Rows 90..140: below the glyphs, where only a shadow offset 80 px down
    // can reach. N32 is BGRA on the platforms this runs on, and red is the
    // one channel the black fill never lights, so count strongly red pixels
    // whichever order the bytes are in.
    let mut n = 0;
    for y in 90..140 {
        for x in 0..256 {
            let i = ((y * info.width() + x) * 4) as usize;
            let (a, b) = (px[i], px[i + 2]);
            let red = a.max(b) > 200 && a.min(b) < 80 && px[i + 1] < 80;
            n += red as usize;
        }
    }
    n
}

/// The shadow lands at its offset, in its own colour, under the fill.
#[test]
fn a_shadow_paints_at_its_offset() {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    let plain = TextStyle::builder().size(40.0).color(Color::new(0, 0, 0, 255)).build();
    let shadowed = TextStyle::builder()
        .size(40.0)
        .color(Color::new(0, 0, 0, 255))
        .shadow(Shadow {
            color: Color::new(220, 0, 0, 255),
            blur: 0.0,
            offset_x: 0.0,
            offset_y: 80.0,
        })
        .build();
    assert_eq!(red_under(&mut env, &plain), 0, "no shadow, nothing below");
    assert!(red_under(&mut env, &shadowed) > 50, "the shadow is drawn 80 px down");
}
