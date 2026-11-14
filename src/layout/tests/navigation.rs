use super::*;

/// Workspace 0 holds three columns (focused on the first); workspace 1 holds one.
fn layout() -> (Layout<TestWindow>, Output) {
    let mut layout = check_ops([
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
        Op::MoveWindowToWorkspaceDown(false),
        Op::FocusColumnLeft,
        Op::FocusColumnLeft,
        Op::CompleteAnimations,
    ]);
    layout.refresh(true);
    let output = layout.outputs().next().unwrap().clone();
    (layout, output)
}

fn workspace(layout: &Layout<TestWindow>, idx: usize) -> &Workspace<TestWindow> {
    layout
        .workspaces()
        .find(|(_, ws_idx, _)| *ws_idx == idx)
        .unwrap()
        .2
}

fn view_pos(layout: &Layout<TestWindow>, idx: usize) -> f64 {
    workspace(layout, idx).scrolling().view_pos()
}

fn scrolling_gesture(layout: &Layout<TestWindow>, idx: usize) -> bool {
    workspace(layout, idx)
        .scrolling()
        .view_offset()
        .is_gesture()
}

fn render_idx(layout: &Layout<TestWindow>, output: &Output) -> f64 {
    layout
        .monitor_for_output(output)
        .unwrap()
        .workspace_render_idx()
}

fn active_idx(layout: &Layout<TestWindow>, output: &Output) -> usize {
    layout
        .monitor_for_output(output)
        .unwrap()
        .active_workspace_idx()
}

/// Feeds motion like a touchpad would, refreshing and advancing frames in between.
fn stroke(
    layout: &mut Layout<TestWindow>,
    nav: &mut Navigation,
    output: &Output,
    deltas: &[(f64, f64)],
) -> bool {
    let mut ongoing = true;
    for (i, &(dx, dy)) in deltas.iter().enumerate() {
        let timestamp = Duration::from_millis(1000 + i as u64 * 16);
        ongoing = layout.navigation_update(nav, output, Point::from((dx, dy)), timestamp);
        layout.refresh(true);
        Op::AdvanceAnimations { msec_delta: 16 }.apply(layout);
        layout.verify_invariants();
    }
    ongoing
}

fn settle(layout: &mut Layout<TestWindow>, nav: &Navigation) {
    layout.navigation_end(nav);
    Op::CompleteAnimations.apply(layout);
    layout.refresh(true);
    layout.verify_invariants();
}

#[test]
fn locked_stroke_follows_only_its_first_axis() {
    let (mut layout, output) = layout();
    let start_view = view_pos(&layout, 0);
    let mut nav = Navigation::new(true);
    // Mostly vertical first motion, then sideways: stays a workspace switch.
    assert!(stroke(
        &mut layout,
        &mut nav,
        &output,
        &[(2., 30.), (60., 30.), (60., 30.)]
    ));
    assert!(render_idx(&layout, &output) > 0.);
    assert_eq!(view_pos(&layout, 0), start_view);
    assert!(!scrolling_gesture(&layout, 0));
    settle(&mut layout, &nav);
}

#[test]
fn locked_horizontal_stroke_never_switches_workspaces() {
    let (mut layout, output) = layout();
    let start_view = view_pos(&layout, 0);
    let mut nav = Navigation::new(true);
    assert!(stroke(
        &mut layout,
        &mut nav,
        &output,
        &[(40., 2.), (40., 80.), (40., 80.)]
    ));
    assert_eq!(render_idx(&layout, &output), 0.);
    assert_ne!(view_pos(&layout, 0), start_view);
    settle(&mut layout, &nav);
    assert_eq!(active_idx(&layout, &output), 0);
}

#[test]
fn unlocked_stroke_moves_both_axes_at_once() {
    let (mut layout, output) = layout();
    let start_view = view_pos(&layout, 0);
    let mut nav = Navigation::new(false);
    // Diagonal motion that stays nearer workspace 0: both its columns and the workspace
    // position move within the same stroke, across refreshes.
    assert!(stroke(
        &mut layout,
        &mut nav,
        &output,
        &[(40., 20.), (40., 20.), (40., 20.)]
    ));
    let idx = render_idx(&layout, &output);
    assert!(idx > 0. && idx < 0.5, "workspace position moved: {idx}");
    assert_ne!(view_pos(&layout, 0), start_view, "columns scrolled");
    assert!(scrolling_gesture(&layout, 0));
    settle(&mut layout, &nav);
    assert!(!scrolling_gesture(&layout, 0));
}

#[test]
fn unlocked_horizontal_scrolling_follows_the_workspace_in_view() {
    let (mut layout, output) = layout();
    let start_view_1 = view_pos(&layout, 1);
    let mut nav = Navigation::new(false);
    // Go most of the way down to workspace 1, then keep moving sideways.
    stroke(&mut layout, &mut nav, &output, &[(0., 120.), (0., 120.)]);
    assert!(render_idx(&layout, &output) > 0.5);
    // Workspace 1 is not active until the switch ends, yet its columns are being scrolled
    // and a refresh must not cancel that.
    assert_eq!(active_idx(&layout, &output), 0);
    assert!(scrolling_gesture(&layout, 1));
    assert!(!scrolling_gesture(&layout, 0));
    stroke(&mut layout, &mut nav, &output, &[(-60., 0.), (-60., 0.)]);
    assert!(scrolling_gesture(&layout, 1));
    assert_ne!(view_pos(&layout, 1), start_view_1);

    settle(&mut layout, &nav);
    assert_eq!(active_idx(&layout, &output), 1);
    assert!(!scrolling_gesture(&layout, 0));
    assert!(!scrolling_gesture(&layout, 1));
}

#[test]
fn unlocked_strokes_can_repeat_and_reverse() {
    let (mut layout, output) = layout();
    for (dy, expected) in [(250., 1), (-250., 0)] {
        let mut nav = Navigation::new(false);
        stroke(
            &mut layout,
            &mut nav,
            &output,
            &[(30., dy / 2.), (30., dy / 2.)],
        );
        settle(&mut layout, &nav);
        assert_eq!(active_idx(&layout, &output), expected);
    }
}

#[test]
fn ending_an_unstarted_stroke_is_harmless() {
    let (mut layout, _) = layout();
    for lock_axis in [true, false] {
        settle(&mut layout, &Navigation::new(lock_axis));
    }
}

#[test]
fn closing_windows_mid_stroke_keeps_invariants() {
    let (mut layout, output) = layout();
    let mut nav = Navigation::new(false);
    stroke(&mut layout, &mut nav, &output, &[(30., 120.), (30., 120.)]);
    check_ops_on_layout(&mut layout, [Op::CloseWindow(3), Op::CloseWindow(1)]);
    stroke(&mut layout, &mut nav, &output, &[(30., -40.)]);
    settle(&mut layout, &nav);
}
