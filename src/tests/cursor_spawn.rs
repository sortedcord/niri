use niri_config::Config;
use niri_ipc::PositionChange;
use smithay::utils::{Logical, Point, Size};

use super::client::ClientId;
use super::*;

const CONFIG: &str = r#"
layout {
    border {
        off
    }
}

window-rule {
    open-floating true
    float-spawn mode="smart-edge" width=400 height=300 distance=10
}
"#;

/// Fixture with the given outputs, each `(n, (w, h))`.
fn fixture(outputs: &[(u8, (u16, u16))]) -> Fixture {
    let mut f = Fixture::with_config(Config::parse_mem(CONFIG).unwrap());
    for &(n, size) in outputs {
        f.add_output(n, size);
    }
    f
}

/// Global logical position of `local` inside output `n`.
fn global_at(f: &mut Fixture, n: u8, local: (f64, f64)) -> Point<f64, Logical> {
    let output = f.niri_output(n);
    let geo = f.niri().global_space.output_geometry(&output).unwrap();
    Point::from((
        f64::from(geo.loc.x) + local.0,
        f64::from(geo.loc.y) + local.1,
    ))
}

/// Opens a floating window that maps with the cursor at `cursor`.
fn open_at(f: &mut Fixture, cursor: Point<f64, Logical>) -> ClientId {
    // Placement is decided when the window maps, so the cursor must be in place first.
    f.niri_state().move_cursor(cursor);

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    // Adopt the size the rule asks for, as a well-behaved client would.
    window.set_size(400, 300);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();
    id
}

/// Output name, top-left position, and tile size of the window, in its output's coordinates.
/// Every test here opens a single window.
fn placed(f: &mut Fixture) -> (String, Point<f64, Logical>, Size<f64, Logical>) {
    let (mon, _, ws) = f
        .niri()
        .layout
        .workspaces()
        .find(|(_, _, ws)| ws.tiles_with_render_positions().next().is_some())
        .unwrap();
    let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
    (mon.unwrap().output_name().clone(), pos, tile.tile_size())
}

#[test]
fn smart_edge_in_the_middle_of_the_screen_centers_on_the_cursor() {
    let mut f = fixture(&[(1, (1920, 1080))]);
    let cursor = global_at(&mut f, 1, (960., 540.));
    open_at(&mut f, cursor);
    let (_, pos, size) = placed(&mut f);
    assert_eq!(size, Size::from((400., 300.)));
    assert_eq!(pos, Point::from((760., 390.)));
}

#[test]
fn bottom_right_corner_sits_at_the_cursor() {
    let mut f = fixture(&[(1, (1920, 1080))]);
    let cursor = global_at(&mut f, 1, (1915., 1075.));
    open_at(&mut f, cursor);
    let (_, pos, _) = placed(&mut f);
    assert_eq!(pos, Point::from((1505., 765.)));
}

#[test]
fn left_edge_extends_up_from_the_cursor_when_it_fits() {
    let mut f = fixture(&[(1, (1920, 1080))]);
    let cursor = global_at(&mut f, 1, (3., 540.));
    open_at(&mut f, cursor);
    let (_, pos, _) = placed(&mut f);
    assert_eq!(pos, Point::from((13., 230.)));
}

#[test]
fn opens_on_the_output_under_the_cursor_with_positions_in_that_output() {
    let mut f = fixture(&[(1, (1920, 1080)), (2, (1280, 720))]);
    let cursor = global_at(&mut f, 2, (640., 360.));
    open_at(&mut f, cursor);
    let (name, pos, _) = placed(&mut f);
    assert_eq!(name, f.niri_output(2).name());
    // Centered on the middle of the 1280 × 720 output.
    assert_eq!(pos, Point::from((440., 210.)));
}

#[test]
fn cursor_position_is_used_only_at_open() {
    let mut f = fixture(&[(1, (1920, 1080))]);
    let cursor = global_at(&mut f, 1, (1915., 1075.));
    let id = open_at(&mut f, cursor);

    // The user moves the window, then tiles and floats it again with the cursor somewhere else.
    // It should return to where the user left it, not to a spot computed from the cursor.
    f.niri().layout.move_floating_window(
        None,
        PositionChange::SetFixed(100.),
        PositionChange::SetFixed(120.),
        false,
    );
    f.niri().layout.toggle_window_floating(None);
    f.double_roundtrip(id);
    f.niri_state().move_cursor(Point::from((10., 10.)));
    f.niri().layout.toggle_window_floating(None);
    f.double_roundtrip(id);
    f.niri_complete_animations();

    let (_, pos, _) = placed(&mut f);
    assert_eq!(pos, Point::from((100., 120.)));
}
