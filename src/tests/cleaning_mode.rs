use niri_config::Config;
use smithay::backend::input::{KeyState, Keycode};
use smithay::utils::Point;

use super::client::ClientId;
use super::*;
use crate::niri::KeyboardFocus;

// XKB keycodes are evdev codes plus 8. These are physical keys on a US layout.
const KEY_C: u32 = 46 + 8;
const KEY_L: u32 = 38 + 8;
const KEY_E: u32 = 18 + 8;
const KEY_A: u32 = 30 + 8;
const KEY_R: u32 = 19 + 8;
const KEY_X: u32 = 45 + 8;
const KEY_LEFTSHIFT: u32 = 42 + 8;
const KEY_CAPSLOCK: u32 = 58 + 8;

fn us_config() -> Config {
    let config = r#"
input {
    keyboard {
        xkb {
            layout "us"
        }
    }
}
"#;
    Config::parse_mem(config).unwrap()
}

fn fixture() -> Fixture {
    let mut f = Fixture::with_config(us_config());
    f.add_output(1, (1920, 1080));
    f
}

fn press(f: &mut Fixture, key: u32) {
    f.niri_state()
        .cleaning_mode_key(Keycode::new(key), KeyState::Pressed, 0);
}

fn release(f: &mut Fixture, key: u32) {
    f.niri_state()
        .cleaning_mode_key(Keycode::new(key), KeyState::Released, 0);
}

fn tap(f: &mut Fixture, key: u32) {
    press(f, key);
    release(f, key);
}

fn active(f: &mut Fixture) -> bool {
    f.niri().cleaning_mode.is_active()
}

/// Opens a window and lets it take the keyboard focus. Returns the client.
fn open_window(f: &mut Fixture) -> ClientId {
    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(200, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();
    id
}

fn keyboard_has_focus(f: &mut Fixture) -> bool {
    f.niri()
        .seat
        .get_keyboard()
        .unwrap()
        .current_focus()
        .is_some()
}

#[test]
fn typing_clear_leaves_cleaning_mode() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();
    assert!(active(&mut f));

    for key in [KEY_C, KEY_L, KEY_E, KEY_A] {
        tap(&mut f, key);
        assert!(active(&mut f), "still active before the last letter");
    }
    press(&mut f, KEY_R);
    assert!(!active(&mut f));
}

#[test]
fn a_wrong_key_starts_over() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();

    for key in [KEY_C, KEY_L, KEY_E, KEY_X, KEY_A, KEY_R] {
        tap(&mut f, key);
    }
    assert!(
        active(&mut f),
        "clear was interrupted, so it must not leave"
    );
    assert_eq!(f.niri().cleaning_mode.progress(), 0);

    for key in [KEY_C, KEY_L, KEY_E, KEY_A, KEY_R] {
        tap(&mut f, key);
    }
    assert!(!active(&mut f));
}

#[test]
fn shift_and_caps_lock_do_not_change_the_letters() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();

    press(&mut f, KEY_LEFTSHIFT);
    tap(&mut f, KEY_C);
    release(&mut f, KEY_LEFTSHIFT);
    tap(&mut f, KEY_CAPSLOCK);
    for key in [KEY_L, KEY_E, KEY_A, KEY_R] {
        tap(&mut f, key);
    }
    assert!(!active(&mut f));
}

#[test]
fn key_releases_alone_never_advance() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();
    for key in [KEY_C, KEY_L, KEY_E, KEY_A, KEY_R] {
        release(&mut f, key);
    }
    assert!(active(&mut f));
}

#[test]
fn entering_again_keeps_the_typed_progress() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();
    tap(&mut f, KEY_C);
    tap(&mut f, KEY_L);
    f.niri_state().enter_cleaning_mode();
    assert_eq!(f.niri().cleaning_mode.progress(), 2);
}

#[test]
fn leaving_without_typing_works_for_a_script() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();
    f.niri_state().exit_cleaning_mode();
    assert!(!active(&mut f));
    // Leaving when not active is a no-op.
    f.niri_state().exit_cleaning_mode();
    assert!(!active(&mut f));
}

#[test]
fn keyboard_focus_is_taken_from_the_window_and_given_back() {
    let mut f = fixture();
    open_window(&mut f);
    f.niri_state().update_keyboard_focus();
    assert!(keyboard_has_focus(&mut f), "the window starts focused");

    f.niri_state().enter_cleaning_mode();
    assert_eq!(f.niri().keyboard_focus, KeyboardFocus::CleaningMode);
    assert!(!keyboard_has_focus(&mut f), "clients lose keyboard focus");

    f.niri_state().exit_cleaning_mode();
    assert!(matches!(
        f.niri().keyboard_focus,
        KeyboardFocus::Layout { surface: Some(_) }
    ));
    assert!(keyboard_has_focus(&mut f), "the window gets it back");
}

#[test]
fn pointer_hits_no_window_while_active() {
    let mut f = fixture();
    open_window(&mut f);

    // The window sits in the layout's top-left area; find a point inside it.
    let inside = {
        let ws = f.niri().layout.active_workspace().unwrap();
        let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
        pos + tile.tile_size().to_point().downscale(2.)
    };
    assert!(f.niri().window_under(inside).is_some());
    assert!(f.niri().contents_under(inside).window.is_some());

    f.niri_state().enter_cleaning_mode();
    assert!(f.niri().window_under(inside).is_none());
    assert!(f.niri().workspace_under(false, inside).is_none());
    let contents = f.niri().contents_under(inside);
    assert!(contents.window.is_none() && contents.surface.is_none());

    f.niri_state().exit_cleaning_mode();
    assert!(f.niri().window_under(inside).is_some());
}

#[test]
fn entering_cancels_a_view_gesture_that_was_in_progress() {
    let mut f = fixture();
    open_window(&mut f);
    let output = f.niri_output(1);
    f.niri()
        .layout
        .view_offset_gesture_begin(&output, None, true);
    let in_gesture = |f: &mut Fixture| {
        f.niri()
            .layout
            .workspaces()
            .any(|(_, _, ws)| ws.scrolling().is_view_gesture_active())
    };
    assert!(in_gesture(&mut f));

    f.niri_state().enter_cleaning_mode();
    assert!(
        !in_gesture(&mut f),
        "its end event would otherwise be dropped"
    );
}

#[test]
fn keys_still_held_when_leaving_have_their_release_swallowed() {
    let mut f = fixture();
    f.niri_state().enter_cleaning_mode();
    // A stale entry from before cleaning mode must not survive entering.
    f.niri().suppressed_keys.insert(Keycode::new(KEY_X));
    f.niri_state().exit_cleaning_mode();
    f.niri_state().enter_cleaning_mode();
    assert!(f.niri().suppressed_keys.is_empty());

    for key in [KEY_C, KEY_L, KEY_E, KEY_A] {
        tap(&mut f, key);
    }
    // The finger is still on "r" when the mode ends.
    press(&mut f, KEY_R);
    assert!(!active(&mut f));
    assert!(f.niri().suppressed_keys.contains(&Keycode::new(KEY_R)));
    assert_eq!(f.niri().suppressed_keys.len(), 1);
}

#[test]
fn entering_while_a_key_bind_suppression_is_pending_clears_it() {
    let mut f = fixture();
    f.niri().suppressed_keys.insert(Keycode::new(KEY_C));
    f.niri_state().enter_cleaning_mode();
    assert!(f.niri().suppressed_keys.is_empty());
}

#[test]
fn exit_and_enter_actions_work_through_do_action() {
    use niri_config::Action;

    let mut f = fixture();
    f.niri_state().do_action(Action::EnterCleaningMode, false);
    assert!(active(&mut f));
    f.niri_state().do_action(Action::ExitCleaningMode, false);
    assert!(!active(&mut f));
}

#[test]
fn the_cursor_position_is_untouched_by_cleaning_mode() {
    let mut f = fixture();
    f.niri_state().move_cursor(Point::from((300., 200.)));
    f.niri_state().enter_cleaning_mode();
    f.niri_state().exit_cleaning_mode();
    let pos = f.niri().cursor_position().unwrap();
    assert_eq!((pos.x, pos.y), (300., 200.));
}
