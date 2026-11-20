//! Where egui's floating layers (tooltips, menus, drop-down lists) fall on
//! the page: the page's native view is cut there so they show through.

use application::ports::Viewport;
use egui::{Context, Order, Rect};

/// The page's area in physical pixels, from its rectangle in points.
pub fn page_viewport(page: Rect, pixels_per_point: f32) -> Viewport {
    Viewport {
        x: (page.min.x * pixels_per_point).round() as i32,
        y: (page.min.y * pixels_per_point).round() as i32,
        width: (page.width() * pixels_per_point).round().max(1.0) as u32,
        height: (page.height() * pixels_per_point).round().max(1.0) as u32,
    }
}

/// The parts of `layers` (rectangles in points) over `page`, in physical
/// pixels relative to the page's top-left corner, rounded outwards. Empty
/// rectangles and those outside the page give nothing.
pub fn page_holes(
    layers: impl IntoIterator<Item = Rect>,
    page: Rect,
    pixels_per_point: f32,
) -> Vec<Viewport> {
    let area = page_viewport(page, pixels_per_point);
    let (width, height) = (area.width as i32, area.height as i32);
    layers
        .into_iter()
        .filter(|r| r.is_finite() && r.is_positive())
        .filter_map(|r| {
            let edge = |v: f32, origin: i32, size: i32, round: fn(f32) -> f32| {
                (round(v * pixels_per_point) as i32)
                    .saturating_sub(origin)
                    .clamp(0, size)
            };
            let left = edge(r.min.x, area.x, width, f32::floor);
            let top = edge(r.min.y, area.y, height, f32::floor);
            let right = edge(r.max.x, area.x, width, f32::ceil);
            let bottom = edge(r.max.y, area.y, height, f32::ceil);
            (right > left && bottom > top).then(|| Viewport {
                x: left,
                y: top,
                width: (right - left) as u32,
                height: (bottom - top) as u32,
            })
        })
        .collect()
}

/// The rectangles of the floating layers egui painted this frame (call it
/// once the interface is drawn). Panels sit beside the page: not them.
pub fn floating_layers(ctx: &Context) -> Vec<Rect> {
    let layers = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| matches!(l.order, Order::Foreground | Order::Tooltip))
            .filter_map(|l| Some((l, m.area_rect(l.id)?)))
            .collect::<Vec<_>>()
    });
    // `visible_layer_ids` keeps last frame's too: only what is painted now.
    ctx.graphics(|g| {
        layers
            .into_iter()
            .filter(|(l, _)| g.get(*l).is_some_and(|p| !p.is_empty()))
            .map(|(_, r)| r)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    fn vp(x: i32, y: i32, width: u32, height: u32) -> Viewport {
        Viewport {
            x,
            y,
            width,
            height,
        }
    }

    const PAGE: Rect = Rect {
        min: egui::Pos2 { x: 0.0, y: 100.0 },
        max: egui::Pos2 { x: 800.0, y: 600.0 },
    };

    #[test]
    fn relative_to_the_page_corner() {
        let holes = page_holes([rect(10.0, 120.0, 60.0, 140.0)], PAGE, 1.0);
        assert_eq!(holes, vec![vp(10, 20, 50, 20)]);
    }

    #[test]
    fn a_rectangle_across_the_page_edge_is_cut() {
        // A tooltip under a toolbar button, starting above the page.
        let holes = page_holes([rect(700.0, 90.0, 900.0, 130.0)], PAGE, 1.0);
        assert_eq!(holes, vec![vp(700, 0, 100, 30)]);
    }

    #[test]
    fn nothing_outside_the_page() {
        let outside = [rect(10.0, 10.0, 60.0, 90.0), rect(10.0, 10.0, 60.0, 100.0)];
        assert!(page_holes(outside, PAGE, 1.0).is_empty());
        assert!(page_holes([rect(810.0, 200.0, 900.0, 300.0)], PAGE, 1.0).is_empty());
    }

    #[test]
    fn empty_rectangles_give_nothing() {
        assert!(page_holes([rect(10.0, 200.0, 10.0, 300.0)], PAGE, 1.0).is_empty());
        assert!(page_holes([Rect::NOTHING], PAGE, 1.0).is_empty());
    }

    #[test]
    fn physical_pixels_rounded_outwards() {
        // At 150 %: the page starts at y = 150 px.
        let holes = page_holes([rect(10.3, 120.3, 20.1, 130.1)], PAGE, 1.5);
        // 15.45 -> 15, 180.45 -> 180, 30.15 -> 31, 195.15 -> 196.
        assert_eq!(holes, vec![vp(15, 30, 16, 16)]);
    }

    #[test]
    fn the_page_area_matches_its_viewport() {
        let page = rect(0.5, 100.5, 800.5, 600.5);
        assert_eq!(page_viewport(page, 1.5), vp(1, 151, 1200, 750));
        let holes = page_holes([page], page, 1.5);
        assert_eq!(holes, vec![vp(0, 0, 1200, 750)]);
    }

    #[test]
    fn one_hole_per_layer() {
        let layers = [
            rect(0.0, 100.0, 10.0, 110.0),
            rect(20.0, 150.0, 30.0, 160.0),
        ];
        assert_eq!(
            page_holes(layers, PAGE, 2.0),
            vec![vp(0, 0, 20, 20), vp(40, 100, 20, 20)]
        );
    }
}
