use std::time::Duration;

use niri_config::Config;
use smithay::input::pointer::{Focus, GrabStartData as PointerGrabStartData};
use smithay::utils::SERIAL_COUNTER;

use super::*;
use crate::input::configured_swipe::{SwipeBegin, SwipeBlock};
use crate::input::pick_window_grab::PickWindowGrab;
use crate::utils::get_monotonic_time;

const GESTURES: &str = r#"
touchpad-gesture fingers=3 modifiers="any" {
    ignore
}
touchpad-gesture fingers=4 {
    navigate
}
"#;

fn fixture(config: &str) -> Fixture {
    let mut f = Fixture::with_config(Config::parse_mem(config).unwrap());
    f.add_output(1, (1920, 1080));
    f
}

/// Holds a pointer grab, like an open popup menu or an unfinished drag-and-drop would.
fn grab_pointer(f: &mut Fixture) {
    let state = f.niri_state();
    let pointer = state.niri.seat.get_pointer().unwrap();
    let start_data = PointerGrabStartData {
        focus: None,
        button: 0,
        location: pointer.current_location(),
    };
    pointer.set_grab(
        state,
        PickWindowGrab::new(start_data),
        SERIAL_COUNTER.next_serial(),
        Focus::Clear,
    );
}

fn release_pointer(f: &mut Fixture) {
    let state = f.niri_state();
    let pointer = state.niri.seat.get_pointer().unwrap();
    pointer.unset_grab(
        state,
        SERIAL_COUNTER.next_serial(),
        get_monotonic_time().as_millis() as u32,
    );
}

/// Runs a whole swipe through the configured gesture handling. Returns how it began, and
/// whether every update and the end were consumed (kept away from the built-in gestures).
fn swipe(f: &mut Fixture, fingers: u32) -> (SwipeBegin, bool) {
    let state = f.niri_state();
    state.configured_swipe_end(true);
    let begin = state.configured_swipe_begin(fingers);
    if begin != SwipeBegin::Claimed {
        return (begin, false);
    }
    let mut consumed = true;
    for i in 0..10 {
        consumed &=
            state.configured_swipe_update(0., 40., false, Duration::from_millis(1000 + i * 16));
    }
    consumed &= state.configured_swipe_end(false);
    (begin, consumed)
}

fn active_workspace_idx(f: &mut Fixture) -> usize {
    let output = f.niri_output(1);
    f.niri()
        .layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_idx()
}

#[test]
fn configured_fingers_never_fall_back_to_stock_gestures_during_a_pointer_grab() {
    let mut f = fixture(GESTURES);
    grab_pointer(&mut f);
    assert_eq!(
        f.niri_state().configured_swipe_block(),
        Some(SwipeBlock::PointerGrab)
    );

    // Both configured finger counts are claimed (and ignored) rather than handed to the
    // built-in three-finger workspace swipe or four-finger overview.
    for fingers in [3, 4] {
        let (begin, consumed) = swipe(&mut f, fingers);
        assert_eq!(begin, SwipeBegin::Claimed, "{fingers} fingers");
        assert!(consumed, "{fingers}-finger updates must not leak through");
    }
    assert!(!f.niri().layout.is_overview_open());
    assert_eq!(active_workspace_idx(&mut f), 0);
}

#[test]
fn an_ignored_swipe_does_not_run_its_configured_action_either() {
    let mut f = fixture(GESTURES);
    grab_pointer(&mut f);
    // A long vertical four-finger swipe would switch workspaces through `navigate`.
    let (begin, _) = swipe(&mut f, 4);
    assert_eq!(begin, SwipeBegin::Claimed);
    f.niri_complete_animations();
    assert_eq!(active_workspace_idx(&mut f), 0);
}

#[test]
fn unconfigured_fingers_still_get_the_stock_gestures_during_a_grab() {
    // Only four fingers are configured, so three fingers keep niri's own behavior.
    let mut f = fixture(
        r#"
touchpad-gesture fingers=4 {
    navigate
}
"#,
    );
    grab_pointer(&mut f);
    assert_eq!(swipe(&mut f, 3).0, SwipeBegin::Unclaimed);
    assert_eq!(swipe(&mut f, 4).0, SwipeBegin::Claimed);
}

#[test]
fn without_any_touchpad_gestures_nothing_changes() {
    let mut f = fixture("");
    grab_pointer(&mut f);
    for fingers in [3, 4] {
        assert_eq!(swipe(&mut f, fingers).0, SwipeBegin::Unclaimed);
    }
    release_pointer(&mut f);
    for fingers in [3, 4] {
        assert_eq!(swipe(&mut f, fingers).0, SwipeBegin::Unclaimed);
    }
}

#[test]
fn configured_gestures_resume_once_the_grab_ends() {
    let mut f = fixture(GESTURES);
    grab_pointer(&mut f);
    assert_eq!(swipe(&mut f, 4).0, SwipeBegin::Claimed);
    release_pointer(&mut f);
    assert_eq!(f.niri_state().configured_swipe_block(), None);

    // Now the same swipe goes to its configured binding again.
    let (begin, consumed) = swipe(&mut f, 4);
    assert_eq!(begin, SwipeBegin::Claimed);
    assert!(consumed);
}

#[test]
fn nothing_blocks_by_default() {
    let mut f = fixture(GESTURES);
    assert_eq!(f.niri_state().configured_swipe_block(), None);
}
