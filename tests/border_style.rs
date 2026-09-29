//! A border's `style` reaches the pixels. `dashed` and `dotted` were
//! parsed into `BorderSide::style` and then drawn solid, because no painter
//! read the field — so these tests count gaps along a painted edge, on
//! both the sharp per-side path and the rounded outline path.

use ogham::skia::SkiaEnv;
use ogham::widget::style::{Border, BorderSide, BorderStyle, Color, Corners};
use ogham::widget::RenderContext;

fn side(style: BorderStyle) -> BorderSide {
    BorderSide { width: 2.0, color: Color::new(0, 0, 0, 255), style }
}

/// How many times the top edge changes between ink and paper, read along
/// its middle row, away from the corners.
fn breaks_along_the_top(style: BorderStyle, corners: Corners) -> usize {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 64)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    env.surface.canvas().clear(ogham::skia_safe::Color::WHITE);
    let s = side(style);
    let border = Border::new(s.clone(), s.clone(), s.clone(), s);
    env.draw_border(&border, 10.0, 10.0, 220.0, 40.0, &corners);
    let info = env.surface.image_info();
    let mut px = vec![0u8; (info.width() * info.height() * 4) as usize];
    assert!(env.surface.read_pixels(&info, &mut px, (info.width() * 4) as usize, (0, 0)));
    let y = 11;
    let dark = |x: i32| px[((y * info.width() + x) * 4) as usize] < 128;
    (40..200).zip(41..201).filter(|&(a, b)| dark(a) != dark(b)).count()
}

#[test]
fn a_solid_border_is_unbroken() {
    assert_eq!(breaks_along_the_top(BorderStyle::Solid, Corners::identity()), 0);
}

#[test]
fn a_dashed_border_has_gaps_on_either_path() {
    assert!(breaks_along_the_top(BorderStyle::Dashed, Corners::identity()) > 10);
    assert!(breaks_along_the_top(BorderStyle::Dashed, Corners::all_round(6.0)) > 10);
}

#[test]
fn a_dotted_border_has_gaps() {
    assert!(breaks_along_the_top(BorderStyle::Dotted, Corners::identity()) > 10);
}

/// The dash is set on a paint every later draw shares, so a dashed border
/// must not leak into the next solid one.
#[test]
fn a_dash_does_not_outlive_its_border() {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 64)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    let dashed = side(BorderStyle::Dashed);
    env.draw_border(
        &Border::new(dashed.clone(), dashed.clone(), dashed.clone(), dashed),
        0.0, 0.0, 50.0, 20.0, &Corners::all_round(4.0),
    );
    assert!(env.paint.path_effect().is_none());
}
