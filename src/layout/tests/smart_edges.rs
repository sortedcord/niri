use super::*;

fn options(enabled: bool) -> Options {
    Options {
        layout: niri_config::Layout {
            full_width_to_edges: enabled,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn add(id: usize) -> Op {
    Op::AddWindow {
        params: TestWindowParams::new(id),
    }
}

fn settle(layout: &mut Layout<TestWindow>, ids: &[usize]) {
    for &id in ids {
        Op::Communicate(id).apply(layout);
    }
    Op::CompleteAnimations.apply(layout);
    layout.update_render_elements(None);
    layout.verify_invariants();
}

fn tile(layout: &Layout<TestWindow>, id: usize) -> &Tile<TestWindow> {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .find(|tile| *tile.window().id() == id)
        .unwrap()
}

fn tile_x(layout: &Layout<TestWindow>, id: usize) -> f64 {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| *tile.window().id() == id)
        .unwrap()
        .1
        .x
}

fn progress(layout: &Layout<TestWindow>, id: usize) -> f64 {
    tile(layout, id).smart_edges().unwrap().progress
}

/// A full-width window (1) followed by a half-width window (2), with 1 focused.
fn full_then_half(enabled: bool) -> Layout<TestWindow> {
    let mut layout = check_ops_with_options(
        options(enabled),
        [
            Op::AddOutput(1),
            add(1),
            Op::MaximizeColumn,
            add(2),
            Op::FocusColumnLeft,
        ],
    );
    settle(&mut layout, &[1, 2]);
    layout
}

#[test]
fn focused_full_width_window_is_sized_to_edges_but_keeps_its_slot() {
    let layout = full_then_half(true);
    let tile = tile(&layout, 1);
    let smart = tile
        .smart_edges()
        .expect("full-width single window must be eligible");

    // 1280x720 output, default 16 px gaps, no layer-shell exclusive zones.
    assert_eq!(smart.slot, Size::from((1248., 688.)));
    assert_eq!(
        smart.edges,
        Rectangle::new(Point::from((-16., -16.)), Size::from((1280., 720.)))
    );
    assert_eq!(
        tile.window().requested_size(),
        Some(Size::from((1280, 720)))
    );
    assert_eq!(tile.window_size(), Size::from((1280., 720.)));
    // The layout keeps using the gapped slot, so the next column does not move.
    assert_eq!(tile.tile_size(), smart.slot);
    assert_eq!(progress(&layout, 1), 1.);
    assert_eq!(tile.window_loc(), Point::from((-16., -16.)));
    assert_eq!(tile.expanded_progress(), 1.);
    assert!(
        tile.smart_edges_render_scale().is_none(),
        "drawn 1:1 when fully expanded"
    );
}

#[test]
fn neighbor_column_positions_are_identical_with_and_without_the_feature() {
    let on = full_then_half(true);
    let off = full_then_half(false);
    assert!(tile(&off, 1).smart_edges().is_none());
    assert_eq!(tile_x(&on, 2), tile_x(&off, 2));
    assert_eq!(tile(&on, 1).tile_size(), tile(&off, 1).tile_size());
}

#[test]
fn expansion_follows_horizontal_scrolling_continuously() {
    let mut layout = full_then_half(true);
    let output = layout.outputs().next().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);

    let mut previous = progress(&layout, 1);
    assert_eq!(previous, 1.);
    let mut saw_partial = false;
    for step in 1..=20 {
        layout.view_offset_gesture_update(64., Duration::from_millis(step * 10), false);
        layout.update_render_elements(None);
        let current = progress(&layout, 1);
        assert!(
            current <= previous + 1e-9,
            "scrolling away must not re-expand"
        );
        if current > 0. && current < 1. {
            saw_partial = true;
            let (rect, scale) = tile(&layout, 1).smart_edges_window_rect().unwrap();
            // Strictly between the gapped slot and the edges, never a jump.
            assert!(rect.size.w > 1248. && rect.size.w < 1280.);
            assert!(scale.x < 1. && scale.x > 0.97);
            assert!(tile(&layout, 1).smart_edges_render_scale().is_some());
        }
        previous = current;
    }
    assert!(saw_partial);
    assert_eq!(previous, 0., "scrolled a full column away");
    let tile = tile(&layout, 1);
    let (rect, _) = tile.smart_edges_window_rect().unwrap();
    assert_eq!(
        rect,
        Rectangle::new(Point::from((0., 0.)), Size::from((1248., 688.)))
    );
    layout.verify_invariants();
}

#[test]
fn scrolling_back_restores_full_expansion() {
    let mut layout = full_then_half(true);
    Op::FocusColumnRight.apply(&mut layout);
    Op::CompleteAnimations.apply(&mut layout);
    layout.update_render_elements(None);
    let away = progress(&layout, 1);
    assert!(away < 1.);
    Op::FocusColumnLeft.apply(&mut layout);
    Op::CompleteAnimations.apply(&mut layout);
    layout.update_render_elements(None);
    assert_eq!(progress(&layout, 1), 1.);
    // The window never resized while navigating: no client configure churn.
    assert_eq!(
        tile(&layout, 1).window().requested_size(),
        Some(Size::from((1280, 720)))
    );
}

#[test]
fn half_width_and_stacked_columns_are_not_eligible() {
    let layout = full_then_half(true);
    assert!(tile(&layout, 2).smart_edges().is_none());

    let mut layout = check_ops_with_options(
        options(true),
        [
            Op::AddOutput(1),
            add(1),
            add(2),
            Op::FocusColumnLeft,
            Op::ConsumeWindowIntoColumn,
            Op::MaximizeColumn,
        ],
    );
    settle(&mut layout, &[1, 2]);
    assert!(tile(&layout, 1).smart_edges().is_none());
    assert!(tile(&layout, 2).smart_edges().is_none());
}

#[test]
fn feature_is_disabled_by_default() {
    let mut layout = check_ops([Op::AddOutput(1), add(1), Op::MaximizeColumn]);
    settle(&mut layout, &[1]);
    assert!(tile(&layout, 1).smart_edges().is_none());
    assert_eq!(tile(&layout, 1).window_size(), Size::from((1248., 688.)));
}

#[test]
fn leaving_full_width_keeps_the_slot_until_the_client_resizes() {
    let mut layout = full_then_half(true);
    let slot = tile(&layout, 1).smart_edges().unwrap().slot;
    let next_x = tile_x(&layout, 2);

    Op::MaximizeColumn.apply(&mut layout); // Toggle back to half width.
    let smart = tile(&layout, 1).smart_edges().unwrap();
    assert!(smart.leaving);
    // The oversized edge window must not momentarily widen the column.
    assert_eq!(tile(&layout, 1).tile_size(), slot);
    layout.verify_invariants();

    settle(&mut layout, &[1]);
    assert!(tile(&layout, 1).smart_edges().is_none());
    assert!(tile(&layout, 1).tile_size().w < slot.w);
    assert!(tile_x(&layout, 2) < next_x);
}

#[test]
fn consuming_a_second_window_ends_eligibility() {
    let mut layout = full_then_half(true);
    Op::ConsumeOrExpelWindowRight { id: None }.apply(&mut layout);
    settle(&mut layout, &[1, 2]);
    assert!(tile(&layout, 1).smart_edges().is_none());
}

#[test]
fn input_maps_exactly_into_the_expanded_and_scaled_window() {
    // Fully expanded: the window is drawn 1:1, covering the gaps from the output origin.
    let layout = full_then_half(true);
    let tile_pos = Point::from((16., 16.));
    assert_eq!(tile_x(&layout, 1), tile_pos.x);
    let first = tile(&layout, 1);
    let screen = Point::from((5., 5.));
    let local = screen - tile_pos;
    assert_eq!(first.tile_to_window_point(local), Point::from((5., 5.)));
    let surface = local - first.input_buffer_origin(local) + first.window().buf_loc().to_f64();
    assert_eq!(surface, Point::from((5., 5.)));

    // Partially expanded: points map through the presentation scale, and the input origin is
    // chosen so the pointer's surface-local position matches what is drawn under it.
    let mut layout = full_then_half(true);
    let output = layout.outputs().next().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);
    layout.view_offset_gesture_update(400., Duration::from_millis(10), false);
    layout.update_render_elements(None);
    let partial = tile(&layout, 1);
    let (rect, scale) = partial.smart_edges_window_rect().unwrap();
    assert!(scale.x < 1. && scale.y < 1.);
    let window = partial.window_size();
    for (fx, fy) in [(0.1, 0.1), (0.5, 0.5), (0.9, 0.75)] {
        let local = partial.window_loc() + Point::from((rect.size.w * fx, rect.size.h * fy));
        let mapped = partial.tile_to_window_point(local);
        assert!(
            (mapped.x - window.w * fx).abs() < 1e-6,
            "x maps to {mapped:?}"
        );
        assert!(
            (mapped.y - window.h * fy).abs() < 1e-6,
            "y maps to {mapped:?}"
        );
        // The rounded origin stays within a physical pixel of the ideal blended rectangle.
        assert!((partial.window_loc().x - rect.loc.x).abs() <= 0.5);
        let surface =
            local - partial.input_buffer_origin(local) + partial.window().buf_loc().to_f64();
        assert!((surface.x - mapped.x).abs() < 1e-9 && (surface.y - mapped.y).abs() < 1e-9);
    }
}

#[test]
fn fullscreen_takes_over_and_restores_edges() {
    let mut layout = full_then_half(true);
    Op::FullscreenWindow(1).apply(&mut layout);
    settle(&mut layout, &[1]);
    assert!(tile(&layout, 1).smart_edges().is_none());
    Op::FullscreenWindow(1).apply(&mut layout);
    settle(&mut layout, &[1]);
    assert!(tile(&layout, 1).smart_edges().is_some());
    assert_eq!(tile(&layout, 1).window_size(), Size::from((1280., 720.)));
}

#[test]
fn moving_an_expanded_window_to_floating_releases_the_edges() {
    let mut layout = full_then_half(true);
    Op::ToggleWindowFloating { id: Some(1) }.apply(&mut layout);
    settle(&mut layout, &[1]);
    let tile = tile(&layout, 1);
    assert!(
        tile.smart_edges().is_none(),
        "floating windows never use edge expansion"
    );
    // Floating layout sees the real window size, not a stale column slot.
    assert_eq!(tile.tile_size(), tile.window_size());
    assert_eq!(tile.window_loc(), Point::from((0., 0.)));
}

#[test]
fn edge_expansion_survives_moving_the_column_to_another_workspace() {
    let mut layout = full_then_half(true);
    Op::MoveColumnToWorkspaceDown(true).apply(&mut layout);
    settle(&mut layout, &[1]);
    assert!(tile(&layout, 1).smart_edges().is_some());
    assert_eq!(progress(&layout, 1), 1.);
    assert_eq!(tile(&layout, 1).window_size(), Size::from((1280., 720.)));
}
