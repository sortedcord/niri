use super::*;

fn fixture() -> Layout<TestWindow> {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::Communicate(1),
        Op::Communicate(2),
        Op::Communicate(3),
        Op::CompleteAnimations,
    ])
}

fn slots(layout: &Layout<TestWindow>) -> Vec<(usize, (usize, usize))> {
    let mut slots: Vec<_> = layout
        .workspaces()
        .flat_map(|(_, _, workspace)| {
            workspace
                .tiles_with_ipc_layouts()
                .filter_map(|(tile, geometry)| {
                    geometry
                        .pos_in_scrolling_layout
                        .map(|position| (*tile.window().id(), position))
                })
        })
        .collect();
    slots.sort();
    slots
}

fn ghost_count(layout: &Layout<TestWindow>) -> usize {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .filter(|tile| tile.is_carry_ghost())
        .count()
}

fn ghost_opacity(layout: &Layout<TestWindow>) -> f32 {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .find_map(|tile| tile.carry_ghost_opacity())
        .unwrap()
}

#[test]
fn lift_size_position_and_ghost_follow_finger_distance_without_an_initial_jump() {
    let mut layout = fixture();
    let original = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| tile.window().id() == &1)
        .map(|(tile, location, _)| Rectangle::new(location, tile.animated_tile_size()))
        .unwrap();
    let slots_before = slots(&layout);
    let output = layout.carry_begin(&1, 0.85).unwrap();
    assert_eq!(layout.carry_pickup_progress(), Some(0.));
    assert_eq!(layout.carry_preview_rect(), Some(original));
    assert_eq!(ghost_opacity(&layout), 1.);
    layout.carry_motion(30.);
    let first = layout.carry_preview_rect().unwrap();
    assert_eq!(layout.carry_pickup_progress(), Some(0.25));
    assert!(first.size.w < original.size.w && first.size.w > original.size.w * 0.85);
    assert!(ghost_opacity(&layout) < 1. && ghost_opacity(&layout) > 0.25);
    // Navigation is not delayed until the pickup finishes.
    layout.view_offset_gesture_begin(&output, None, false);
    let before_pan = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2
        .scrolling()
        .view_pos();
    layout.view_offset_gesture_update(100., Duration::from_millis(10), false);
    let after_pan = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2
        .scrolling()
        .view_pos();
    assert_ne!(before_pan, after_pan);
    assert_eq!(layout.carry_preview_rect(), Some(first));
    assert_eq!(slots(&layout), slots_before);
    layout.carry_motion(30.);
    let halfway = layout.carry_preview_rect().unwrap();
    assert_eq!(layout.carry_pickup_progress(), Some(0.5));
    assert!(halfway.size.w < first.size.w);
    layout.carry_motion(60.);
    let complete = layout.carry_preview_rect().unwrap();
    assert_eq!(layout.carry_pickup_progress(), Some(1.));
    assert!((complete.size.w - original.size.w * 0.85).abs() < 0.001);
    assert_eq!(
        complete.loc + complete.size.to_point().downscale(2.),
        Point::from((640., 360.))
    );
    assert_eq!(ghost_opacity(&layout), 0.25);
    assert_eq!(slots(&layout), slots_before);
    layout.verify_invariants();
}

#[test]
fn partial_lift_finishes_smoothly_after_motion_pauses() {
    let mut layout = fixture();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(30.);
    Op::AdvanceAnimations { msec_delta: 50 }.apply(&mut layout);
    assert_eq!(layout.carry_pickup_progress(), Some(0.25));
    assert!(layout.are_animations_ongoing(None));
    Op::AdvanceAnimations { msec_delta: 60 }.apply(&mut layout);
    assert_eq!(layout.carry_pickup_progress(), Some(0.25));
    Op::AdvanceAnimations { msec_delta: 50 }.apply(&mut layout);
    let in_flight = layout.carry_pickup_progress().unwrap();
    assert!(in_flight > 0.25 && in_flight < 1.);
    Op::AdvanceAnimations { msec_delta: 2000 }.apply(&mut layout);
    assert_eq!(layout.carry_pickup_progress(), Some(1.));
    assert!(!layout.carry_is_animating(None));
    assert!(layout.carry_is_active());
    assert_eq!(ghost_count(&layout), 1);
}

#[test]
fn finger_up_settles_immediately_and_resuming_motion_does_not_jump_backwards() {
    let mut layout = fixture();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(30.);
    layout.carry_finish_pickup();
    Op::AdvanceAnimations { msec_delta: 40 }.apply(&mut layout);
    let before_resume = layout.carry_pickup_progress().unwrap();
    assert!(before_resume > 0.25 && before_resume < 1.);
    layout.carry_motion(1.);
    assert!(layout.carry_pickup_progress().unwrap() >= before_resume);
    layout.carry_end(true);
    assert!(!layout.carry_is_active());
    assert!(!layout.carry_is_animating(None));
    assert_eq!(ghost_count(&layout), 0);
    layout.verify_invariants();
}

#[test]
fn pickup_distance_controls_how_far_a_finger_must_move() {
    let mut layout = fixture();
    layout.carry_begin_with_distance(&1, 0.85, 240.).unwrap();
    layout.carry_motion(60.);
    assert_eq!(layout.carry_pickup_progress(), Some(0.25));
    layout.carry_end(true);
    layout.carry_begin_with_distance(&1, 0.85, 120.).unwrap();
    layout.carry_motion(60.);
    assert_eq!(layout.carry_pickup_progress(), Some(0.5));
}

fn single_window() -> Layout<TestWindow> {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::Communicate(1),
        Op::CompleteAnimations,
    ])
}

fn hidden_landing_count(layout: &Layout<TestWindow>) -> usize {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .filter(|tile| tile.is_carry_drop_hidden())
        .count()
}

#[test]
fn release_moves_and_grows_the_preview_over_300_ms_then_reveals_the_real_tile() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    let released = layout.carry_preview_rect().unwrap();
    layout.carry_end(false);
    assert!(!layout.carry_is_active());
    assert_eq!(layout.carry_drop_rect(), Some(released));
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(hidden_landing_count(&layout), 1);
    assert!(layout.are_animations_ongoing(None));
    layout.verify_invariants();
    Op::AdvanceAnimations { msec_delta: 150 }.apply(&mut layout);
    let halfway = layout.carry_drop_rect().unwrap();
    assert!(halfway.size.w > released.size.w);
    assert!(halfway.size.w < released.size.w / 0.85);
    assert!(halfway.loc.x < released.loc.x);
    assert_eq!(hidden_landing_count(&layout), 1);
    Op::AdvanceAnimations { msec_delta: 149 }.apply(&mut layout);
    let almost = layout.carry_drop_rect().unwrap();
    assert!((almost.size.w - released.size.w / 0.85).abs() < 0.01);
    Op::AdvanceAnimations { msec_delta: 1 }.apply(&mut layout);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    assert_eq!(layout.windows().count(), 1);
    layout.verify_invariants();
}

#[test]
fn longer_drop_duration_is_respected_and_zero_disables_it() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end_with_duration(false, 600);
    Op::AdvanceAnimations { msec_delta: 300 }.apply(&mut layout);
    assert!(layout.carry_drop_rect().is_some());
    Op::AdvanceAnimations { msec_delta: 300 }.apply(&mut layout);
    assert!(layout.carry_drop_rect().is_none());
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end_with_duration(false, 0);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
}

#[test]
fn partial_pickup_release_has_no_geometry_jump_at_landing_start() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(30.);
    let partial = layout.carry_preview_rect().unwrap();
    layout.carry_end(false);
    assert_eq!(layout.carry_drop_rect(), Some(partial));
    assert!(layout.carry_drop_opacity().unwrap() >= 0.5);
    Op::AdvanceAnimations { msec_delta: 300 }.apply(&mut layout);
    assert_eq!(hidden_landing_count(&layout), 0);
}

#[test]
fn a_destination_resize_during_landing_keeps_the_preview_geometry_continuous() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(false);
    let before_resize = layout.carry_drop_rect().unwrap();
    layout.set_column_width(SizeChange::SetFixed(900));
    Op::Communicate(1).apply(&mut layout);
    // A client resize cannot change the initial preview rectangle at t=0.
    assert_eq!(layout.carry_drop_rect(), Some(before_resize));
    Op::AdvanceAnimations { msec_delta: 150 }.apply(&mut layout);
    let midway = layout.carry_drop_rect().unwrap();
    assert!(midway.size.w > before_resize.size.w);
    assert!(midway.size.h > before_resize.size.h);
    Op::AdvanceAnimations { msec_delta: 150 }.apply(&mut layout);
    assert_eq!(hidden_landing_count(&layout), 0);
    layout.verify_invariants();
}

#[test]
fn disabled_animations_do_not_leave_an_invisible_landing_tile() {
    let mut layout = single_window();
    let mut options = (*layout.options).clone();
    options.animations.off = true;
    layout.update_options(options);
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(false);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    layout.verify_invariants();
}

#[test]
fn cancel_still_restores_immediately_without_a_landing_animation() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(true);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    assert_eq!(ghost_count(&layout), 0);
}

#[test]
fn closing_a_landing_window_cleans_up_the_hidden_tile_and_overlay() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(false);
    check_ops_on_layout(&mut layout, [Op::CloseWindow(1)]);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    assert_eq!(layout.windows().count(), 0);
}

#[test]
fn unplug_during_landing_keeps_the_committed_window_visible() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(false);
    check_ops_on_layout(&mut layout, [Op::RemoveOutput(1), Op::AddOutput(1)]);
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    assert_eq!(layout.windows().count(), 1);
}

#[test]
fn a_new_pickup_safely_interrupts_the_previous_landing() {
    let mut layout = single_window();
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    layout.carry_end(false);
    Op::AdvanceAnimations { msec_delta: 100 }.apply(&mut layout);
    layout.carry_begin(&1, 0.85).unwrap();
    assert!(layout.carry_drop_rect().is_none());
    assert_eq!(hidden_landing_count(&layout), 0);
    assert_eq!(ghost_count(&layout), 1);
    layout.verify_invariants();
}

#[test]
fn carry_preserves_every_original_slot_and_marks_one_ghost() {
    let mut layout = fixture();
    let before = slots(&layout);
    layout.carry_begin(&1, 0.85).unwrap();
    assert_eq!(slots(&layout), before);
    assert_eq!(ghost_count(&layout), 1);
    assert!(layout.interactive_move.is_none());
    assert!(layout.carry_is_active());
    assert_eq!(layout.overview_zoom(), 1.);
    layout.verify_invariants();
}

#[test]
fn finger_up_navigation_end_keeps_carry_and_ghost_alive_for_next_stroke() {
    let mut layout = fixture();
    let before = slots(&layout);
    let output = layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    let preview = layout.carry_preview_rect().unwrap();
    assert_eq!(
        preview.loc + preview.size.to_point().downscale(2.),
        Point::from((640., 360.))
    );
    for (index, delta) in [300., -200., 150.].into_iter().enumerate() {
        layout.view_offset_gesture_begin(&output, None, true);
        assert!(layout
            .view_offset_gesture_update(
                delta,
                Duration::from_millis(100 + index as u64 * 100),
                true
            )
            .is_some());
        layout.view_offset_gesture_end(Some(true));
        layout.refresh(true);
        layout.advance_animations();
        assert_eq!(slots(&layout), before);
        assert_eq!(ghost_count(&layout), 1);
        assert_eq!(layout.carry_preview_rect(), Some(preview));
        assert!(layout.carry_is_active());
        layout.verify_invariants();
    }
    layout.carry_end(true);
    assert_eq!(slots(&layout), before);
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(layout.focus().unwrap().id(), &1);
}

#[test]
fn vertical_swipe_moves_the_workspace_under_the_centered_copy() {
    let mut layout = fixture();
    let output = layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_motion(120.);
    let origin = layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_ref()
        .id();
    let preview = layout.carry_preview_rect();
    let before = slots(&layout);
    layout.workspace_switch_gesture_begin(&output, true);
    layout.workspace_switch_gesture_update(400., Duration::from_millis(100), true);
    layout.workspace_switch_gesture_end(Some(true));
    Op::CompleteAnimations.apply(&mut layout);
    layout.refresh(true);
    let target = layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_ref()
        .id();
    assert_ne!(target, origin);
    assert_eq!(layout.focus().unwrap().id(), &1);
    assert_eq!(layout.carry_preview_rect(), preview);
    assert_eq!(slots(&layout), before);
    assert_eq!(ghost_count(&layout), 1);
    assert!(layout.carry_is_active());
    layout.verify_invariants();
    layout.carry_end(false);
    assert_eq!(
        layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2
            .id(),
        target
    );
    assert_eq!(ghost_count(&layout), 0);
}

#[test]
fn escape_cancel_does_not_remove_or_reinsert_the_window() {
    let mut layout = fixture();
    let before = slots(&layout);
    let output = layout.carry_begin(&1, 0.85).unwrap();
    layout.workspace_switch_gesture_begin(&output, true);
    layout.workspace_switch_gesture_update(350., Duration::from_millis(100), true);
    layout.workspace_switch_gesture_end(Some(true));
    layout.carry_end(true);
    assert!(!layout.carry_is_active());
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(slots(&layout), before);
    assert_eq!(layout.focus().unwrap().id(), &1);
    layout.verify_invariants();
}

#[test]
fn drop_onto_the_original_ghost_does_not_duplicate_the_window() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::Communicate(1),
        Op::CompleteAnimations,
    ]);
    layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_end(false);
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(layout.windows().count(), 1);
    assert_eq!(layout.focus().unwrap().id(), &1);
    layout.verify_invariants();
}

#[test]
fn drop_on_another_workspace_removes_only_the_original_ghost() {
    let mut layout = fixture();
    let origin = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2
        .id();
    let output = layout.carry_begin(&1, 0.85).unwrap();
    // Navigate to the empty workspace underneath the fixed centered preview.
    layout.switch_workspace_down();
    Op::CompleteAnimations.apply(&mut layout);
    let target = layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_ref()
        .id();
    assert_ne!(target, origin);
    layout.carry_end(false);
    let actual = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2
        .id();
    assert_eq!(actual, target);
    assert_eq!(layout.windows().count(), 3);
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(layout.focus().unwrap().id(), &1);
    layout.verify_invariants();
}

#[test]
fn drop_uses_the_preview_anchor_even_when_removing_source_shifts_column_indices() {
    let mut layout = fixture();
    let output = layout.carry_begin(&1, 0.85).unwrap();
    layout.activate_window(&3);
    layout.center_column();
    Op::CompleteAnimations.apply(&mut layout);
    let x = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_render_positions())
        .find(|(tile, _, _)| tile.window().id() == &3)
        .unwrap()
        .1
        .x;
    // Put the gap just before C exactly at the fixed center anchor. Removing
    // source A must not turn "before C" into "after C" as indices shift.
    layout.view_offset_gesture_begin(&output, None, false);
    layout.view_offset_gesture_update(x - 8. - 640., Duration::from_millis(100), false);
    let center = Point::from((640., 360.));
    let workspace = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&3))
        .unwrap()
        .2;
    assert_eq!(
        workspace.scrolling_insert_position(center),
        InsertPosition::NewColumn(2)
    );
    layout.carry_end(false);
    let mut positions: Vec<_> = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles_with_ipc_layouts())
        .map(|(tile, geometry)| {
            (
                geometry.pos_in_scrolling_layout.unwrap(),
                *tile.window().id(),
            )
        })
        .collect();
    positions.sort();
    assert_eq!(
        positions.into_iter().map(|(_, id)| id).collect::<Vec<_>>(),
        vec![2, 1, 3]
    );
    assert_eq!(ghost_count(&layout), 0);
    layout.verify_invariants();
}

#[test]
fn pickup_rejects_a_concurrent_native_drag() {
    let mut layout = fixture();
    let output = layout.carry_begin(&1, 0.85).unwrap();
    assert!(!layout.interactive_move_begin(2, &output, Point::from((640., 360.))));
    assert!(layout.carry_begin(&2, 0.85).is_none());
    layout.carry_end(true);
    layout.verify_invariants();
}

#[test]
fn closing_carried_window_clears_carry_without_affecting_other_windows() {
    let mut layout = fixture();
    layout.carry_begin(&1, 0.85).unwrap();
    check_ops_on_layout(&mut layout, [Op::CloseWindow(1)]);
    assert!(!layout.carry_is_active());
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(layout.windows().count(), 2);
}

#[test]
fn unplug_cancels_carry_without_losing_its_tile() {
    let mut layout = fixture();
    layout.carry_begin(&1, 0.85).unwrap();
    check_ops_on_layout(&mut layout, [Op::RemoveOutput(1), Op::AddOutput(1)]);
    assert!(!layout.carry_is_active());
    assert_eq!(ghost_count(&layout), 0);
    assert_eq!(layout.windows().count(), 3);
}

/// Three windows (focused on the last), with columns wide enough to overflow the view.
fn wide_fixture() -> Layout<TestWindow> {
    let mut layout = fixture();
    for id in [1, 2, 3] {
        check_ops_on_layout(
            &mut layout,
            [
                Op::FocusWindow(id),
                Op::SetColumnWidth(SizeChange::SetProportion(60.)),
                Op::Communicate(id),
            ],
        );
    }
    check_ops_on_layout(&mut layout, [Op::FocusWindow(3), Op::CompleteAnimations]);
    layout
}

fn carry_view_pos(layout: &Layout<TestWindow>) -> f64 {
    layout.active_workspace().unwrap().scrolling().view_pos()
}

/// One carry navigation stroke of horizontal finger motion, then finger-up and settle.
fn carry_stroke(layout: &mut Layout<TestWindow>, output: &Output, dx: f64, steps: u64) {
    let mut nav = Navigation::new(true);
    for i in 0..steps {
        let timestamp = Duration::from_millis(1000 + i * 16);
        layout.navigation_update(&mut nav, output, Point::from((dx, 0.)), timestamp);
        layout.refresh(true);
        Op::AdvanceAnimations { msec_delta: 16 }.apply(layout);
        layout.verify_invariants();
    }
    layout.navigation_end(&nav);
    Op::CompleteAnimations.apply(layout);
    layout.refresh(true);
    layout.verify_invariants();
}

fn carry_insert_position(layout: &Layout<TestWindow>, output: &Output) -> InsertPosition {
    let center = output_size(output).to_point().downscale(2.);
    layout
        .active_workspace()
        .unwrap()
        .scrolling_insert_position(center)
}

fn column_order(layout: &Layout<TestWindow>) -> Vec<usize> {
    let mut slots = slots(layout);
    slots.sort_by_key(|(_, (col, row))| (*col, *row));
    slots.into_iter().map(|(id, _)| id).collect()
}

#[test]
fn carry_view_comes_to_rest_past_the_first_column() {
    let mut layout = wide_fixture();
    let output = layout.carry_begin(&3, 0.85).unwrap();
    layout.carry_finish_pickup();
    // Swipe far toward the start: plain gestures would snap back onto the first column.
    carry_stroke(&mut layout, &output, -60., 30);
    let gaps = 16.;
    let half_view = output_size(&output).w / 2.;
    assert_eq!(carry_view_pos(&layout), -gaps - half_view);
    assert_eq!(
        carry_insert_position(&layout, &output),
        InsertPosition::NewColumn(0)
    );
    // It stays there across refreshes and frames while still carrying.
    for _ in 0..10 {
        layout.refresh(true);
        Op::AdvanceAnimations { msec_delta: 16 }.apply(&mut layout);
    }
    assert_eq!(carry_view_pos(&layout), -gaps - half_view);

    layout.carry_end_with_duration(false, 0);
    Op::CompleteAnimations.apply(&mut layout);
    layout.refresh(true);
    layout.verify_invariants();
    assert_eq!(column_order(&layout), vec![3, 1, 2]);
    // The view returns to normal bounds around the dropped window.
    assert!(carry_view_pos(&layout) >= -gaps);
}

#[test]
fn carry_view_comes_to_rest_past_the_last_column() {
    let mut layout = wide_fixture();
    check_ops_on_layout(&mut layout, [Op::FocusWindow(1), Op::CompleteAnimations]);
    let output = layout.carry_begin(&1, 0.85).unwrap();
    layout.carry_finish_pickup();
    carry_stroke(&mut layout, &output, 60., 30);
    assert_eq!(
        carry_insert_position(&layout, &output),
        InsertPosition::NewColumn(3)
    );
    layout.carry_end_with_duration(false, 0);
    Op::CompleteAnimations.apply(&mut layout);
    layout.refresh(true);
    layout.verify_invariants();
    assert_eq!(column_order(&layout), vec![2, 3, 1]);
}

#[test]
fn small_carry_strokes_still_snap_to_inner_columns() {
    let mut layout = wide_fixture();
    let output = layout.carry_begin(&3, 0.85).unwrap();
    layout.carry_finish_pickup();
    // A short slow stroke stays within the columns.
    carry_stroke(&mut layout, &output, -4., 10);
    assert!(carry_view_pos(&layout) >= -16.);
    layout.carry_end(true);
}

#[test]
fn cancel_or_same_slot_drop_returns_the_view_from_past_the_edge() {
    for cancel in [true, false] {
        let mut layout = wide_fixture();
        check_ops_on_layout(&mut layout, [Op::FocusWindow(1), Op::CompleteAnimations]);
        let before = carry_view_pos(&layout);
        let output = layout.carry_begin(&1, 0.85).unwrap();
        layout.carry_finish_pickup();
        carry_stroke(&mut layout, &output, -60., 30);
        assert!(carry_view_pos(&layout) < -16.);
        // Dropping window 1 before the first column is its own slot.
        layout.carry_end_with_duration(cancel, 0);
        Op::CompleteAnimations.apply(&mut layout);
        layout.refresh(true);
        layout.verify_invariants();
        assert_eq!(column_order(&layout), vec![1, 2, 3]);
        assert_eq!(carry_view_pos(&layout), before, "cancel={cancel}");
    }
}

#[test]
fn plain_view_gestures_never_rest_past_the_edges() {
    let mut layout = wide_fixture();
    let output = layout.outputs().next().unwrap().clone();
    carry_stroke(&mut layout, &output, -60., 30);
    assert!(carry_view_pos(&layout) >= -16.);
}
