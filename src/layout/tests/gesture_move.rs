use super::*;

fn layout() -> Layout<TestWindow> {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::CompleteAnimations,
    ])
}

#[test]
fn pickup_is_immediate_and_only_the_window_is_scaled() {
    let mut layout = layout();
    let (output, _) = layout.gesture_move_begin(&1, 0.85).unwrap();
    layout.verify_invariants();
    let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
        panic!("pickup must detach immediately, without the mouse drag threshold");
    };
    assert_eq!(move_.preview_scale, 0.85);
    assert_eq!(layout.overview_zoom(), 1.);
    assert!(!layout.is_overview_open());
    assert!(layout
        .monitors()
        .find(|mon| mon.output() == &output)
        .unwrap()
        .workspace_switch
        .is_some());
    layout.interactive_move_end(&1);
    layout.verify_invariants();
    assert!(layout.interactive_move.is_none());
    assert!(layout.workspaces().any(|(_, _, ws)| ws.has_window(&1)));
}

#[test]
fn gesture_pickup_drops_at_a_new_column_position() {
    let mut layout = layout();
    let (output, center) = layout.gesture_move_begin(&1, 0.8).unwrap();
    let position = Point::from((1279., 360.));
    assert!(layout.interactive_move_update(&1, position - center, output, position));
    layout.interactive_move_end(&1);
    Op::CompleteAnimations.apply(&mut layout);
    layout.verify_invariants();
    assert_eq!(layout.focus().unwrap().id(), &1);
    let (_, _, ws) = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap();
    let mut positions: Vec<_> = ws
        .tiles_with_render_positions()
        .map(|(tile, pos, _)| (*tile.window().id(), pos.x))
        .collect();
    positions.sort_by(|a, b| a.1.total_cmp(&b.1));
    let ids: Vec<_> = positions.into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![2, 1]);
}

#[test]
fn closing_a_picked_up_window_cleans_up_the_move() {
    let mut layout = layout();
    layout.gesture_move_begin(&1, 0.85).unwrap();
    check_ops_on_layout(&mut layout, [Op::CloseWindow(1)]);
    assert!(layout.interactive_move.is_none());
    assert!(!layout.workspaces().any(|(_, _, ws)| ws.has_window(&1)));
}

#[test]
fn unplugging_the_output_during_pickup_keeps_the_window() {
    let mut layout = layout();
    layout.gesture_move_begin(&1, 0.85).unwrap();
    check_ops_on_layout(
        &mut layout,
        [
            Op::RemoveOutput(1),
            Op::InteractiveMoveEnd { window: 1 },
            Op::AddOutput(1),
        ],
    );
    assert!(layout.workspaces().any(|(_, _, ws)| ws.has_window(&1)));
}

#[test]
fn mouse_move_keeps_normal_scale_and_overview_closed() {
    let mut layout = layout();
    let output = layout.outputs().next().unwrap().clone();
    assert!(layout.interactive_move_begin(1, &output, Point::from((100., 100.))));
    layout.interactive_move_update(
        &1,
        Point::from((400., 0.)),
        output,
        Point::from((500., 100.)),
    );
    let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
        panic!("expected an ongoing mouse move");
    };
    assert_eq!(move_.preview_scale, 1.);
    assert!(!layout.is_overview_open());
    layout.interactive_move_end(&1);
    layout.verify_invariants();
}

#[test]
fn bottom_edge_switches_workspace_without_overview() {
    edge_switches_workspace_without_overview(false);
}

#[test]
fn top_edge_switches_workspace_without_overview() {
    edge_switches_workspace_without_overview(true);
}

fn edge_switches_workspace_without_overview(up: bool) {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_name: Some(1),
            layout_config: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(1),
            layout_config: None,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(1),
            ws_name: 1,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(2),
            ws_name: 2,
        },
        Op::FocusWorkspace(1),
        Op::CompleteAnimations,
    ]);
    let window = if up { 2 } else { 1 };
    let destination_name = if up { "ws1" } else { "ws2" };
    layout.activate_window(&window);
    Op::CompleteAnimations.apply(&mut layout);
    let destination = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().map(String::as_str) == Some(destination_name))
        .unwrap()
        .2
        .id();
    let start_idx = layout.monitors().next().unwrap().workspace_render_idx();
    let (output, center) = layout.gesture_move_begin(&window, 0.85).unwrap();
    let edge = Point::from((640., if up { 0. } else { 719. }));
    layout.interactive_move_update(&window, edge - center, output.clone(), edge);
    // Keep the virtual pointer at the lower edge until the next workspace fills
    // the display. Animation frames, not further gesture events, drive scrolling.
    let mut crossed = false;
    for _ in 0..180 {
        layout.refresh(true);
        Op::AdvanceAnimations { msec_delta: 16 }.apply(&mut layout);
        layout.verify_invariants();
        let mon = layout.monitor_for_output(&output).unwrap();
        if (!up && mon.workspace_render_idx() >= start_idx + 1.)
            || (up && mon.workspace_render_idx() <= start_idx - 1.)
        {
            crossed = true;
            break;
        }
    }
    assert!(crossed, "holding at the edge should switch workspaces");
    assert!(!layout.is_overview_open());
    let middle = Point::from((640., 360.));
    layout.interactive_move_update(&window, middle - edge, output, middle);
    layout.interactive_move_end(&window);
    layout.verify_invariants();
    let actual = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&window))
        .unwrap()
        .2
        .id();
    assert_eq!(actual, destination);
}
