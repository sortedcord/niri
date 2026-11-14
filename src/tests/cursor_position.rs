use smithay::utils::Point;

use super::*;

#[test]
fn cursor_position_is_global_and_relative_to_its_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));

    // Outputs are laid out left to right, so the second one is offset by the first's width.
    let second = f.niri_output(2);
    let geo = f.niri().global_space.output_geometry(&second).unwrap();
    assert_ne!(geo.loc.x, 0);

    f.niri_state().move_cursor(Point::from((100., 50.)));
    let pos = f.niri().cursor_position().unwrap();
    assert_eq!((pos.x, pos.y), (100., 50.));
    assert_eq!(pos.output, f.niri_output(1).name());
    assert_eq!((pos.output_x, pos.output_y), (100., 50.));

    let global = Point::from((f64::from(geo.loc.x) + 30.5, f64::from(geo.loc.y) + 40.));
    f.niri_state().move_cursor(global);
    let pos = f.niri().cursor_position().unwrap();
    assert_eq!((pos.x, pos.y), (global.x, global.y));
    assert_eq!(pos.output, second.name());
    assert_eq!((pos.output_x, pos.output_y), (30.5, 40.));
}

#[test]
fn cursor_position_is_none_outside_every_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    // `move_cursor` doesn't clamp, so this places the cursor outside the only output.
    f.niri_state().move_cursor(Point::from((5000., 5000.)));
    assert_eq!(f.niri().cursor_position(), None);
}
