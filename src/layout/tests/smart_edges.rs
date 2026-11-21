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

#[test]
fn partly_visible_full_width_neighbor_rests_with_normal_margins() {
    // Regression: focusing a half-width neighbor leaves the full-width column half on screen.
    // It must rest fully normal, with the same gaps as any other window, not half-expanded.
    let mut layout = full_then_half(true);
    Op::FocusColumnRight.apply(&mut layout);
    Op::CompleteAnimations.apply(&mut layout);
    layout.update_render_elements(None);

    let x = tile_x(&layout, 1);
    assert!(
        x < 16. && x + 1248. > 0.,
        "column 1 must still be partly visible (x = {x})"
    );
    assert_eq!(progress(&layout, 1), 0.);
    let tile = tile(&layout, 1);
    let (rect, _) = tile.smart_edges_window_rect().unwrap();
    assert_eq!(
        rect,
        Rectangle::new(Point::from((0., 0.)), Size::from((1248., 688.)))
    );
    assert_eq!(tile.expanded_progress(), 0.);
    // Same vertical margins as the half-width neighbor.
    let neighbor = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| *tile.window().id() == 2)
        .unwrap();
    let full = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| *tile.window().id() == 1)
        .unwrap();
    assert_eq!(full.1.y + rect.loc.y, neighbor.1.y);
    assert_eq!(rect.size.h, neighbor.0.tile_size().h);
}

#[test]
fn blending_happens_only_near_alignment() {
    let mut layout = full_then_half(true);
    let output = layout.outputs().next().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);
    // 10% of the working width away: partially expanded.
    layout.view_offset_gesture_update(128., Duration::from_millis(10), false);
    layout.update_render_elements(None);
    let near = progress(&layout, 1);
    assert!(near > 0. && near < 1.);
    // 30% away (beyond the 25% blend distance): fully normal even mid-gesture.
    layout.view_offset_gesture_update(256., Duration::from_millis(20), false);
    layout.update_render_elements(None);
    assert_eq!(progress(&layout, 1), 0.);
}

/// Left edge of window 1 on screen: tile render position plus where the window sits in it.
fn window_left(layout: &Layout<TestWindow>, id: usize) -> f64 {
    let (tile, pos, _) = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| *tile.window().id() == id)
        .unwrap();
    pos.x + tile.window_loc().x
}

#[test]
fn maximizing_grows_from_the_windows_own_position_not_the_screen_center() {
    let mut layout = check_ops_with_options(
        options(true),
        [
            Op::AddOutput(1),
            add(1),
            Op::SetColumnWidth(SizeChange::SetProportion(50.)),
        ],
    );
    settle(&mut layout, &[1]);
    let before = window_left(&layout, 1);
    assert!(tile(&layout, 1).smart_edges().is_none());

    // Mod+F, then the client adopts the sizes niri asks for, frame by frame.
    Op::MaximizeColumn.apply(&mut layout);
    let mut lefts = Vec::new();
    for _ in 0..60 {
        Op::Communicate(1).apply(&mut layout);
        layout.update_render_elements(None);
        lefts.push(window_left(&layout, 1));
        Op::AdvanceAnimations { msec_delta: 16 }.apply(&mut layout);
    }
    settle(&mut layout, &[1]);
    let after = window_left(&layout, 1);
    assert_eq!(after, 0., "ends flush with the screen edge");

    // The left edge only travels from its old spot out to the screen edge. It must never jump
    // right, toward the middle of the screen, on the way.
    for (frame, left) in lefts.iter().enumerate() {
        assert!(
            *left <= before + 0.5 && *left >= after - 0.5,
            "frame {frame}: window left edge at {left}, outside {after}..={before} ({lefts:?})"
        );
    }
}

#[test]
fn unmaximizing_shrinks_back_without_jumping_either() {
    let mut layout = check_ops_with_options(
        options(true),
        [
            Op::AddOutput(1),
            add(1),
            Op::SetColumnWidth(SizeChange::SetProportion(50.)),
        ],
    );
    settle(&mut layout, &[1]);
    let half_left = window_left(&layout, 1);
    Op::MaximizeColumn.apply(&mut layout);
    settle(&mut layout, &[1]);
    assert_eq!(window_left(&layout, 1), 0.);

    Op::MaximizeColumn.apply(&mut layout);
    let mut lefts = Vec::new();
    for _ in 0..60 {
        Op::Communicate(1).apply(&mut layout);
        layout.update_render_elements(None);
        lefts.push(window_left(&layout, 1));
        Op::AdvanceAnimations { msec_delta: 16 }.apply(&mut layout);
    }
    settle(&mut layout, &[1]);
    assert_eq!(window_left(&layout, 1), half_left);
    for (frame, left) in lefts.iter().enumerate() {
        assert!(
            (0. - 0.5..=half_left + 0.5).contains(left),
            "frame {frame}: window left edge at {left} ({lefts:?})"
        );
    }
}

/// Two full-width windows side by side (1 then 2), with 1 focused.
fn two_full_width() -> Layout<TestWindow> {
    let mut layout = check_ops_with_options(
        options(true),
        [
            Op::AddOutput(1),
            add(1),
            Op::MaximizeColumn,
            add(2),
            Op::MaximizeColumn,
            Op::FocusColumnLeft,
        ],
    );
    settle(&mut layout, &[1, 2]);
    layout
}

/// The window's visual rectangle on screen (tile render position plus its rect in the tile).
fn window_on_screen(layout: &Layout<TestWindow>, id: usize) -> Rectangle<f64, Logical> {
    let (tile, pos, _) = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| *tile.window().id() == id)
        .unwrap();
    let (rect, _) = tile.smart_edges_window_rect().unwrap();
    Rectangle::new(pos + rect.loc, rect.size)
}

#[test]
fn swiping_between_two_full_width_windows_stays_at_the_edges() {
    let mut layout = two_full_width();
    assert_eq!(progress(&layout, 1), 1.);
    // At rest the neighbor stays out of view: it must not reach over the focused window.
    assert_eq!(window_on_screen(&layout, 1).loc.x, 0.);
    assert_eq!(window_on_screen(&layout, 1).size.w, 1280.);
    assert!(window_on_screen(&layout, 2).loc.x >= 1280.);

    let output = layout.outputs().next().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);
    // One full column is 1248 + 16; travel it in steps.
    for step in 1..=20 {
        layout.view_offset_gesture_update(63.2, Duration::from_millis(step * 10), false);
        layout.update_render_elements(None);
        // No intermediate phase: both stay full height the whole way.
        assert_eq!(progress(&layout, 1), 1., "step {step}");
        assert_eq!(progress(&layout, 2), 1., "step {step}");
        let (a, b) = (window_on_screen(&layout, 1), window_on_screen(&layout, 2));
        assert_eq!(a.loc.y, 0.);
        assert_eq!(a.size.h, 720.);
        // The two windows meet exactly: no gap, no overlap.
        let seam = b.loc.x - (a.loc.x + a.size.w);
        assert!(seam.abs() < 1e-6, "step {step}: seam {seam}");
    }
    // Arrived: window 2 fills the screen exactly like window 1 did.
    let b = window_on_screen(&layout, 2);
    assert!((b.loc.x - 0.).abs() < 1e-6 && (b.size.w - 1280.).abs() < 1e-6);
    layout.verify_invariants();
}

#[test]
fn a_run_of_full_width_windows_still_blends_out_toward_a_normal_neighbor() {
    // 1 and 2 are full width, 3 is half width. Scrolling from 2 to 3 leaves the run as before.
    let mut layout = check_ops_with_options(
        options(true),
        [
            Op::AddOutput(1),
            add(1),
            Op::MaximizeColumn,
            add(2),
            Op::MaximizeColumn,
            add(3),
            Op::FocusColumnLeft,
        ],
    );
    settle(&mut layout, &[1, 2, 3]);
    assert_eq!(progress(&layout, 2), 1.);
    Op::FocusColumnRight.apply(&mut layout);
    Op::CompleteAnimations.apply(&mut layout);
    layout.update_render_elements(None);
    assert_eq!(progress(&layout, 1), 0.);
    assert_eq!(progress(&layout, 2), 0.);
}

fn options_with_top(top: Option<f64>) -> Options {
    let mut options = options(true);
    options.layout.full_width_to_edges_top = top;
    options
}

#[test]
fn edges_top_moves_only_the_top_of_the_edge_rectangle() {
    let mut layout = check_ops_with_options(
        options_with_top(Some(40.)),
        [
            Op::AddOutput(1),
            add(1),
            Op::MaximizeColumn,
            add(2),
            Op::FocusColumnLeft,
        ],
    );
    settle(&mut layout, &[1, 2]);
    let smart = tile(&layout, 1).smart_edges().unwrap();
    // Same slot as always; the edges start 40 px from the top of the output.
    assert_eq!(smart.slot, Size::from((1248., 688.)));
    assert_eq!(
        smart.edges,
        Rectangle::new(Point::from((-16., 24.)), Size::from((1280., 680.)))
    );
    assert_eq!(
        tile(&layout, 1).window().requested_size(),
        Some(Size::from((1280, 680)))
    );
    let rect = window_on_screen(&layout, 1);
    assert_eq!((rect.loc.y, rect.size.h), (40., 680.));
    // The normal neighbor is unaffected.
    assert!(tile(&layout, 2).smart_edges().is_none());
    assert_eq!(tile(&layout, 2).tile_size().h, 688.);
}

#[test]
fn window_rule_overrides_the_edges_top() {
    let mut layout = check_ops_with_options(
        options_with_top(Some(40.)),
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    rules: Some(ResolvedWindowRules {
                        full_width_to_edges_top: Some(0.),
                        ..ResolvedWindowRules::default()
                    }),
                    ..TestWindowParams::new(1)
                },
            },
            Op::MaximizeColumn,
        ],
    );
    settle(&mut layout, &[1]);
    let rect = window_on_screen(&layout, 1);
    assert_eq!((rect.loc.y, rect.size.h), (0., 720.));
}
