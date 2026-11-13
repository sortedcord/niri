//! Modifier-aware swipe bindings, separate from keyboard binds.
//!
//! Parameterized finger/direction matching and separate continuous/discrete
//! dispatch follow julianjc84/niri's feat/configurable-touch-gestures design.

use std::str::FromStr;

use knuffel::errors::DecodeError;
use miette::miette;

use crate::utils::RegexEq;
use crate::{Action, Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Any,
    Horizontal,
    Vertical,
    Up,
    Down,
    Left,
    Right,
}

impl FromStr for Direction {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "any" => Ok(Self::Any),
            "horizontal" => Ok(Self::Horizontal),
            "vertical" => Ok(Self::Vertical),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(miette!("invalid swipe direction: {s}")),
        }
    }
}

impl Direction {
    pub fn matches(self, dx: f64, dy: f64) -> bool {
        let horizontal = dx.abs() > dy.abs();
        match self {
            Self::Any => true,
            Self::Horizontal => horizontal,
            Self::Vertical => !horizontal,
            Self::Left => horizontal && dx < 0.,
            Self::Right => horizontal && dx > 0.,
            Self::Up => !horizontal && dy < 0.,
            Self::Down => !horizontal && dy > 0.,
        }
    }

    pub fn specificity(self) -> u8 {
        match self {
            Self::Any => 0,
            Self::Horizontal | Self::Vertical => 1,
            _ => 2,
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub enum GestureAction {
    /// Pick up the focused window and feed it continuous touchpad deltas.
    DragWindow,
    /// Keep a centered copy and source ghost while gestures navigate underneath.
    /// Release the configured modifiers to commit, or Escape to cancel.
    CarryWindow,
    ScrollView,
    SwitchWorkspace,
    Overview,
    /// Claim the gesture and do nothing: neither a built-in gesture nor the client gets it.
    Ignore,
    /// Hand the whole swipe to the window under the pointer, as a live stream of finger motion,
    /// instead of acting on it. `app-id` is a regular expression; without it every window gets the
    /// swipe. Windows that do not match are treated as with `ignore`. The choice is made when the
    /// fingers land, so `direction` and `threshold` do not apply.
    ForwardToClient(#[knuffel(property(name = "app-id"), str)] Option<RegexEq>),
    /// Run one ordinary niri action once, after the recognition threshold.
    Run(#[knuffel(children)] Vec<Action>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TouchpadGesture {
    pub fingers: u8,
    pub modifiers: Modifiers,
    /// `modifiers="any"`: match regardless of held modifiers, as a fallback when no binding
    /// with the exact held modifiers exists for this finger count.
    pub any_modifiers: bool,
    pub direction: Direction,
    pub threshold: f64,
    pub sensitivity: f64,
    pub preview_scale: f64,
    pub pickup_distance: f64,
    pub drop_duration_ms: u16,
    pub action: GestureAction,
}

#[derive(knuffel::Decode)]
struct RawGesture {
    #[knuffel(property)]
    fingers: u8,
    #[knuffel(property, default)]
    modifiers: String,
    #[knuffel(property, str, default = Direction::Any)]
    direction: Direction,
    #[knuffel(property, default = 16.)]
    threshold: f64,
    #[knuffel(property, default = 1.)]
    sensitivity: f64,
    #[knuffel(property(name = "preview-scale"), default = 0.85)]
    preview_scale: f64,
    #[knuffel(property(name = "pickup-distance"), default = 120.)]
    pickup_distance: f64,
    #[knuffel(property(name = "drop-duration-ms"), default = 300)]
    drop_duration_ms: u16,
    #[knuffel(children)]
    actions: Vec<GestureAction>,
}

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for TouchpadGesture {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let raw = RawGesture::decode_node(node, ctx)?;
        let error = |message: &str| DecodeError::unexpected(node, "gesture", message);
        if !(3..=10).contains(&raw.fingers) {
            return Err(error("fingers must be between 3 and 10"));
        }
        if !raw.threshold.is_finite() || !(1. ..=1000.).contains(&raw.threshold) {
            return Err(error("threshold must be between 1 and 1000"));
        }
        if !raw.sensitivity.is_finite() || !(0.1..=10.).contains(&raw.sensitivity) {
            return Err(error("sensitivity must be between 0.1 and 10"));
        }
        if !raw.preview_scale.is_finite() || !(0.1..=1.).contains(&raw.preview_scale) {
            return Err(error("preview-scale must be between 0.1 and 1.0"));
        }
        if !raw.pickup_distance.is_finite() || !(1. ..=1000.).contains(&raw.pickup_distance) {
            return Err(error("pickup-distance must be between 1 and 1000"));
        }
        if raw.drop_duration_ms > 5000 {
            return Err(error("drop-duration-ms must be between 0 and 5000"));
        }
        let any_modifiers = raw.modifiers.eq_ignore_ascii_case("any");
        let modifiers = if raw.modifiers.is_empty() || any_modifiers {
            Modifiers::empty()
        } else {
            // Reuse niri's modifier parser, including its Mod/Super distinction.
            format!("{}+A", raw.modifiers)
                .parse::<Key>()
                .map_err(|e| DecodeError::conversion(node, e))?
                .modifiers
        };
        if raw.actions.len() != 1 {
            return Err(error("expected exactly one gesture action"));
        }
        let action = raw.actions.into_iter().next().unwrap();
        if matches!(action, GestureAction::CarryWindow) && (modifiers.is_empty() || any_modifiers) {
            return Err(error(
                "carry-window requires a modifier to release for dropping",
            ));
        }
        if matches!(&action, GestureAction::Run(actions) if actions.len() != 1) {
            return Err(error("run must contain exactly one niri action"));
        }
        Ok(Self {
            fingers: raw.fingers,
            modifiers,
            any_modifiers,
            direction: raw.direction,
            threshold: raw.threshold,
            sensitivity: raw.sensitivity,
            preview_scale: raw.preview_scale,
            pickup_distance: raw.pickup_distance,
            drop_duration_ms: raw.drop_duration_ms,
            action,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Vec<TouchpadGesture>, knuffel::Error> {
        knuffel::parse("test.kdl", text)
    }

    #[test]
    fn decode_drag_and_run() {
        let entries = parse(
            r#"
            touchpad-gesture fingers=3 modifiers="Super" preview-scale=0.8 {
                drag-window
            }
            touchpad-gesture fingers=4 direction="up" {
                run { spawn "true"; }
            }
        "#,
        )
        .unwrap();
        assert_eq!(entries[0].modifiers, Modifiers::SUPER);
        assert_eq!(entries[0].preview_scale, 0.8);
        assert_eq!(entries[0].action, GestureAction::DragWindow);
        assert_eq!(entries[1].direction, Direction::Up);
        assert!(matches!(entries[1].action, GestureAction::Run(_)));
    }

    #[test]
    fn progressive_pickup_distance_is_configurable() {
        let entries = parse(
            r#"touchpad-gesture fingers=3 modifiers="Super" pickup-distance=240.0 {
            carry-window
        }"#,
        )
        .unwrap();
        assert_eq!(entries[0].pickup_distance, 240.);
        assert_eq!(entries[0].action, GestureAction::CarryWindow);
    }

    #[test]
    fn landing_duration_is_configurable_and_can_be_disabled() {
        let entries = parse(
            r#"
            touchpad-gesture fingers=3 modifiers="Super" drop-duration-ms=450 { carry-window; }
            touchpad-gesture fingers=4 modifiers="Super" drop-duration-ms=0 { carry-window; }
        "#,
        )
        .unwrap();
        assert_eq!(entries[0].drop_duration_ms, 450);
        assert_eq!(entries[1].drop_duration_ms, 0);
    }

    #[test]
    fn ignore_and_any_modifiers() {
        let entries = parse(
            r#"
            touchpad-gesture fingers=3 modifiers="any" { ignore; }
            touchpad-gesture fingers=4 modifiers="ANY" direction="vertical" { overview; }
            touchpad-gesture fingers=4 { scroll-view; }
        "#,
        )
        .unwrap();
        assert!(entries[0].any_modifiers);
        assert_eq!(entries[0].modifiers, Modifiers::empty());
        assert_eq!(entries[0].action, GestureAction::Ignore);
        assert!(entries[1].any_modifiers);
        assert!(!entries[2].any_modifiers);
    }

    #[test]
    fn forward_to_client_with_and_without_app_id() {
        let entries = parse(
            r#"
            touchpad-gesture fingers=3 modifiers="any" { forward-to-client app-id="^helium$"; }
            touchpad-gesture fingers=4 modifiers="any" { forward-to-client; }
        "#,
        )
        .unwrap();
        match &entries[0].action {
            GestureAction::ForwardToClient(Some(pattern)) => {
                assert!(pattern.0.is_match("helium"));
                assert!(!pattern.0.is_match("helium-other"));
            }
            other => panic!("unexpected action: {other:?}"),
        }
        assert_eq!(entries[1].action, GestureAction::ForwardToClient(None));
    }

    #[test]
    fn forward_to_client_rejects_a_bad_pattern() {
        assert!(
            parse(r#"touchpad-gesture fingers=3 { forward-to-client app-id="(unclosed"; }"#)
                .is_err()
        );
    }

    #[test]
    fn modifiers_and_defaults() {
        let entries = parse(
            r#"touchpad-gesture fingers=3 modifiers="Mod+Shift" {
            scroll-view
        }"#,
        )
        .unwrap();
        assert_eq!(
            entries[0].modifiers,
            Modifiers::COMPOSITOR | Modifiers::SHIFT
        );
        assert_eq!(entries[0].direction, Direction::Any);
        assert_eq!(entries[0].threshold, 16.);
        assert_eq!(entries[0].pickup_distance, 120.);
        assert_eq!(entries[0].drop_duration_ms, 300);
    }

    #[test]
    fn reject_invalid_entries() {
        for text in [
            "touchpad-gesture fingers=2 { drag-window; }",
            "touchpad-gesture fingers=11 { drag-window; }",
            "touchpad-gesture fingers=3 threshold=0 { drag-window; }",
            "touchpad-gesture fingers=3 sensitivity=0 { drag-window; }",
            "touchpad-gesture fingers=3 preview-scale=1.5 { drag-window; }",
            "touchpad-gesture fingers=3 modifiers=\"NoSuchModifier\" { drag-window; }",
            "touchpad-gesture fingers=3 direction=\"diagonal\" { drag-window; }",
            "touchpad-gesture fingers=3 { drag-window; scroll-view; }",
            "touchpad-gesture fingers=3 { run { close-window; quit; }; }",
            "touchpad-gesture fingers=3 { run; }",
            "touchpad-gesture fingers=3 { carry-window; }",
            "touchpad-gesture fingers=3 modifiers=\"any\" { carry-window; }",
            "touchpad-gesture fingers=3 pickup-distance=0 { drag-window; }",
            "touchpad-gesture fingers=3 pickup-distance=1001 { drag-window; }",
            "touchpad-gesture fingers=3 drop-duration-ms=5001 { drag-window; }",
            "touchpad-gesture fingers=3 drop-duration-ms=-1 { drag-window; }",
        ] {
            assert!(parse(text).is_err(), "should reject {text}");
        }
    }

    #[test]
    fn top_level_entries_merge_and_later_binding_overrides() {
        let parsed = crate::Config::parse(
            std::path::Path::new("/tmp/gesture-test.kdl"),
            r#"
            touchpad-gesture fingers=3 modifiers="Mod" { drag-window; }
            touchpad-gesture fingers=4 direction="up" { overview; }
            touchpad-gesture fingers=3 modifiers="Mod" preview-scale=0.7 { drag-window; }
        "#,
        )
        .config
        .unwrap();
        assert_eq!(parsed.touchpad_gestures.len(), 2);
        assert_eq!(parsed.touchpad_gestures[1].preview_scale, 0.7);
    }

    #[test]
    fn direction_matching() {
        assert!(Direction::Any.matches(1., 2.));
        assert!(Direction::Left.matches(-10., 2.));
        assert!(!Direction::Left.matches(10., 2.));
        assert!(Direction::Vertical.matches(2., 10.));
        assert!(Direction::Down.matches(2., 10.));
        assert!(!Direction::Up.matches(2., 10.));
        assert!(Direction::Horizontal.specificity() > Direction::Any.specificity());
        assert!(Direction::Up.specificity() > Direction::Vertical.specificity());
    }
}
