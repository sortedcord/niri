//! Configurable touchpad swipes, with begin/update/end continuous dispatch.
//!
//! The matching/dispatch model is adapted from the approach in
//! julianjc84/niri feat/configurable-touch-gestures. This intentionally ports
//! only touchpad swipes; touchscreen, pinch, and IPC progress are not included.

use std::time::Duration;

use niri_config::touchpad_gestures::{GestureAction, TouchpadGesture};
use niri_config::{ModKey, Modifiers};
use smithay::desktop::Window;
use smithay::input::pointer::{CursorIcon, CursorImageStatus};
use smithay::output::Output;
use smithay::utils::{IsAlive, Logical, Point};

use super::modifiers_from_state;
use crate::layout::Navigation;
use crate::niri::State;
use crate::utils::{output_size, with_toplevel_role};

/// What happened to a new swipe when its fingers landed.
pub(super) enum SwipeBegin {
    /// A configured binding owns the swipe; nothing else may act on it.
    Claimed,
    /// Pass the whole swipe, as a live stream, to the window under the pointer.
    Forward,
    /// No configured binding applies; the built-in gestures may take it.
    Unclaimed,
}

/// Whether a `forward-to-client` action wants a swipe over a window with this app-id.
fn forward_matches(action: &GestureAction, app_id: Option<&str>) -> bool {
    let GestureAction::ForwardToClient(pattern) = action else {
        return false;
    };
    match pattern {
        None => true,
        Some(pattern) => app_id.is_some_and(|app_id| pattern.0.is_match(app_id)),
    }
}

pub struct ConfiguredSwipe {
    candidates: Vec<TouchpadGesture>,
    cumulative: Point<f64, Logical>,
    binding: Option<TouchpadGesture>,
    operation: Operation,
    target: Option<Window>,
    output: Option<Output>,
}

/// Survives finger-up events; only modifier release commits the pickup.
pub struct WindowCarry {
    binding: TouchpadGesture,
    modifiers: Modifiers,
    output: Output,
}

enum Operation {
    Pending,
    Ignored,
    Drag {
        window: Window,
        output: Output,
        position: Point<f64, Logical>,
    },
    Carry {
        navigation: Navigation,
    },
    Navigate {
        navigation: Navigation,
        output: Output,
    },
    View,
    Workspace,
    Overview,
    Run,
}

fn effective_modifiers(mut modifiers: Modifiers, mod_key: ModKey) -> Modifiers {
    if modifiers.contains(Modifiers::COMPOSITOR) {
        modifiers.remove(Modifiers::COMPOSITOR);
        modifiers.insert(mod_key.to_modifiers());
    }
    modifiers
}

#[derive(Debug, PartialEq, Eq)]
enum CarryKey {
    Continue,
    Drop,
    Cancel,
    Consume,
}

fn carry_key(
    required: Modifiers,
    held: Modifiers,
    pressed: bool,
    raw: Option<smithay::input::keyboard::Keysym>,
) -> CarryKey {
    use smithay::input::keyboard::Keysym;
    if pressed && raw == Some(Keysym::Escape) {
        return CarryKey::Cancel;
    }
    if !held.contains(required) {
        return CarryKey::Drop;
    }
    if pressed
        && !matches!(
            raw,
            Some(
                Keysym::Super_L
                    | Keysym::Super_R
                    | Keysym::Shift_L
                    | Keysym::Shift_R
                    | Keysym::Control_L
                    | Keysym::Control_R
                    | Keysym::Alt_L
                    | Keysym::Alt_R
                    | Keysym::ISO_Level3_Shift
                    | Keysym::ISO_Level5_Shift
            )
        )
    {
        CarryKey::Consume
    } else {
        CarryKey::Continue
    }
}

/// Bindings for this finger count with exactly the held modifiers; if there are none,
/// `modifiers="any"` bindings for this finger count.
fn select_candidates(
    gestures: &[TouchpadGesture],
    fingers: u32,
    held: Modifiers,
    mod_key: ModKey,
) -> Vec<TouchpadGesture> {
    let for_fingers = || {
        gestures
            .iter()
            .filter(move |bind| u32::from(bind.fingers) == fingers)
    };
    let exact: Vec<_> = for_fingers()
        .filter(|bind| !bind.any_modifiers && effective_modifiers(bind.modifiers, mod_key) == held)
        .cloned()
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    for_fingers()
        .filter(|bind| bind.any_modifiers)
        .cloned()
        .collect()
}

fn select_binding(
    candidates: &[TouchpadGesture],
    delta: Point<f64, Logical>,
) -> Option<TouchpadGesture> {
    candidates
        .iter()
        .filter(|bind| {
            delta.x.hypot(delta.y) >= bind.threshold && bind.direction.matches(delta.x, delta.y)
        })
        .max_by_key(|bind| bind.direction.specificity())
        .cloned()
}

impl State {
    /// The app-id of the window under the pointer, which is who a forwarded swipe goes to.
    fn pointer_window_app_id(&self) -> Option<String> {
        let mapped = self.niri.window_under_cursor()?;
        with_toplevel_role(mapped.toplevel(), |role| role.app_id.clone())
    }

    pub(super) fn configured_swipe_begin(&mut self, fingers: u32) -> SwipeBegin {
        // Never execute custom actions while locked, picking a screenshot, or
        // another pointer grab is in progress.
        if self.niri.is_locked()
            || self.niri.screenshot_ui.is_open()
            || self.niri.window_mru_ui.is_open()
            || self.is_inhibiting_shortcuts()
            || self.niri.seat.get_pointer().is_some_and(|p| p.is_grabbed())
        {
            return SwipeBegin::Unclaimed;
        }
        let held = modifiers_from_state(self.niri.seat.get_keyboard().unwrap().modifier_state());
        let mod_key = self.backend.mod_key(&self.niri.config.borrow());
        if self.niri.window_carry.as_ref().is_some_and(|carry| {
            !self.niri.layout.carry_is_active() || !held.contains(carry.modifiers)
        }) {
            // Also recover if a modifier release was lost across VT/suspend.
            self.finish_window_carry(true);
        }
        if let Some(carry) = &self.niri.window_carry {
            let matches =
                u32::from(carry.binding.fingers) == fingers && held.contains(carry.modifiers);
            let mut binding = carry.binding.clone();
            binding.direction = niri_config::touchpad_gestures::Direction::Any;
            self.niri.configured_swipe = Some(ConfiguredSwipe {
                candidates: if matches { vec![binding] } else { vec![] },
                cumulative: Point::from((0., 0.)),
                binding: None,
                operation: if matches {
                    Operation::Pending
                } else {
                    Operation::Ignored
                },
                target: None,
                output: Some(carry.output.clone()),
            });
            return SwipeBegin::Claimed;
        }
        let candidates = select_candidates(
            &self.niri.config.borrow().touchpad_gestures,
            fingers,
            held,
            mod_key,
        );
        if candidates.is_empty() {
            return SwipeBegin::Unclaimed;
        }
        // `forward-to-client` is decided now, from the window under the pointer, because a
        // forwarded swipe has to reach the client from its first frame.
        let app_id = self.pointer_window_app_id();
        if candidates
            .iter()
            .any(|bind| forward_matches(&bind.action, app_id.as_deref()))
        {
            self.niri.gesture_swipe_3f_cumulative = None;
            return SwipeBegin::Forward;
        }
        // Over any other window a forward binding behaves like `ignore`.
        let candidates: Vec<_> = candidates
            .into_iter()
            .map(|mut bind| {
                if matches!(bind.action, GestureAction::ForwardToClient(_)) {
                    bind.action = GestureAction::Ignore;
                }
                bind
            })
            .collect();
        let target = self
            .niri
            .layout
            .focus_with_output()
            .map(|(w, _)| w.window.clone());
        let output = self
            .niri
            .layout
            .focus_with_output()
            .map(|(_, o)| o.clone())
            .or_else(|| self.niri.output_under_cursor());
        self.niri.gesture_swipe_3f_cumulative = None;
        self.niri.configured_swipe = Some(ConfiguredSwipe {
            candidates,
            cumulative: Point::from((0., 0.)),
            binding: None,
            operation: Operation::Pending,
            target,
            output,
        });
        SwipeBegin::Claimed
    }

    pub(super) fn configured_swipe_update(
        &mut self,
        dx: f64,
        dy: f64,
        natural_scroll: bool,
        timestamp: Duration,
    ) -> bool {
        let Some(mut swipe) = self.niri.configured_swipe.take() else {
            return false;
        };
        if self.niri.is_locked()
            || self.niri.screenshot_ui.is_open()
            || self.niri.window_mru_ui.is_open()
            || self.is_inhibiting_shortcuts()
        {
            self.finish_configured_operation(&mut swipe, true);
            self.finish_window_carry(true);
            swipe.operation = Operation::Ignored;
            self.niri.configured_swipe = Some(swipe);
            return true;
        }
        let mut delta = Point::from((dx, dy));
        swipe.cumulative += delta;
        if matches!(swipe.operation, Operation::Pending) {
            if let Some(binding) = select_binding(&swipe.candidates, swipe.cumulative) {
                swipe.operation = self.begin_configured_operation(&swipe, &binding);
                swipe.binding = Some(binding);
                delta = swipe.cumulative;
            }
        }
        if let Some(binding) = &swipe.binding {
            let pickup_motion = delta.x.hypot(delta.y);
            delta = delta.upscale(binding.sensitivity);
            let navigation = if natural_scroll {
                delta.upscale(-1.)
            } else {
                delta
            };
            let ongoing = match &mut swipe.operation {
                Operation::Drag {
                    window,
                    output,
                    position,
                } => {
                    if !window.alive() || self.niri.layout.monitor_for_output(output).is_none() {
                        false
                    } else {
                        let size = output_size(output);
                        let previous = *position;
                        position.x = (position.x + delta.x).clamp(0., (size.w - 1.).max(0.));
                        position.y = (position.y + delta.y).clamp(0., (size.h - 1.).max(0.));
                        self.niri.layout.interactive_move_update(
                            window,
                            *position - previous,
                            output.clone(),
                            *position,
                        )
                    }
                }
                Operation::Carry { navigation: nav } => {
                    self.niri.layout.carry_motion(pickup_motion);
                    if let Some(output) = self.niri.layout.carry_output() {
                        self.niri
                            .layout
                            .navigation_update(nav, &output, navigation, timestamp)
                    } else {
                        false
                    }
                }
                Operation::Navigate {
                    navigation: nav,
                    output,
                } => self
                    .niri
                    .layout
                    .navigation_update(nav, output, navigation, timestamp),
                Operation::View => self
                    .niri
                    .layout
                    .view_offset_gesture_update(navigation.x, timestamp, true)
                    .is_some(),
                Operation::Workspace => self
                    .niri
                    .layout
                    .workspace_switch_gesture_update(navigation.y, timestamp, true)
                    .is_some(),
                Operation::Overview => self
                    .niri
                    .layout
                    .overview_gesture_update(-delta.y, timestamp)
                    .is_some(),
                _ => true,
            };
            if !ongoing {
                self.finish_configured_operation(&mut swipe, true);
                swipe.operation = Operation::Ignored;
            }
            self.niri.queue_redraw_all();
        }
        self.niri.configured_swipe = Some(swipe);
        true
    }

    fn begin_configured_operation(
        &mut self,
        swipe: &ConfiguredSwipe,
        binding: &TouchpadGesture,
    ) -> Operation {
        match &binding.action {
            GestureAction::DragWindow => {
                let Some(window) = swipe.target.as_ref().filter(|w| w.alive()) else {
                    return Operation::Ignored;
                };
                let Some((output, position)) = self
                    .niri
                    .layout
                    .gesture_move_begin(window, binding.preview_scale)
                else {
                    return Operation::Ignored;
                };
                self.niri
                    .cursor_manager
                    .set_cursor_image(CursorImageStatus::Named(CursorIcon::Grabbing));
                Operation::Drag {
                    window: window.clone(),
                    output,
                    position,
                }
            }
            GestureAction::CarryWindow => {
                if self.niri.window_carry.is_none() {
                    let Some(window) = swipe.target.as_ref().filter(|window| window.alive()) else {
                        return Operation::Ignored;
                    };
                    let Some(output) = self.niri.layout.carry_begin_with_distance(
                        window,
                        binding.preview_scale,
                        binding.pickup_distance,
                    ) else {
                        return Operation::Ignored;
                    };
                    let mod_key = self.backend.mod_key(&self.niri.config.borrow());
                    self.niri.window_carry = Some(WindowCarry {
                        binding: binding.clone(),
                        modifiers: effective_modifiers(binding.modifiers, mod_key),
                        output,
                    });
                    if let Some(token) = self.niri.bind_repeat_timer.take() {
                        self.niri.event_loop.remove(token);
                    }
                    self.niri
                        .cursor_manager
                        .set_cursor_image(CursorImageStatus::Named(CursorIcon::Grabbing));
                }
                Operation::Carry {
                    navigation: Navigation::new(binding.lock_axis),
                }
            }
            GestureAction::Navigate => {
                let Some(output) = swipe.output.clone() else {
                    return Operation::Ignored;
                };
                Operation::Navigate {
                    navigation: Navigation::new(binding.lock_axis),
                    output,
                }
            }
            GestureAction::ScrollView => {
                let Some(output) = &swipe.output else {
                    return Operation::Ignored;
                };
                self.niri
                    .layout
                    .view_offset_gesture_begin(output, None, true);
                Operation::View
            }
            GestureAction::SwitchWorkspace => {
                let Some(output) = &swipe.output else {
                    return Operation::Ignored;
                };
                self.niri
                    .layout
                    .workspace_switch_gesture_begin(output, true);
                Operation::Workspace
            }
            GestureAction::Overview => {
                self.niri.layout.overview_gesture_begin();
                Operation::Overview
            }
            GestureAction::Run(_) => Operation::Run,
            // Decided when the fingers land (see `SwipeBegin::Forward`); a binding that reaches
            // here did not match the window, so it behaves like `ignore`.
            GestureAction::Ignore | GestureAction::ForwardToClient(_) => Operation::Ignored,
        }
    }

    fn finish_configured_operation(&mut self, swipe: &mut ConfiguredSwipe, cancelled: bool) {
        match &swipe.operation {
            Operation::Drag { window, .. } => {
                // Drop at the last valid insertion point even if libinput cancels
                // (e.g. a finger-count change); never leave a detached window.
                self.niri.layout.interactive_move_end(window);
                self.niri
                    .cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
            }
            Operation::Carry { navigation } => {
                // Keep carry mode, but smoothly finish any partial lift.
                self.niri.layout.carry_finish_pickup();
                self.niri.layout.navigation_end(navigation);
            }
            Operation::Navigate { navigation, .. } => {
                self.niri.layout.navigation_end(navigation);
            }
            Operation::View => {
                self.niri.layout.view_offset_gesture_end(Some(true));
            }
            Operation::Workspace => {
                self.niri.layout.workspace_switch_gesture_end(Some(true));
            }
            Operation::Overview => {
                self.niri.layout.overview_gesture_end();
            }
            Operation::Run
                if !cancelled
                    && !self.niri.is_locked()
                    && !self.niri.screenshot_ui.is_open()
                    && !self.niri.window_mru_ui.is_open()
                    && !self.is_inhibiting_shortcuts() =>
            {
                if let Some(TouchpadGesture {
                    action: GestureAction::Run(actions),
                    ..
                }) = &swipe.binding
                {
                    for action in actions {
                        self.do_action(action.clone(), false);
                    }
                }
            }
            _ => (),
        }
        self.niri.queue_redraw_all();
    }

    fn finish_window_carry(&mut self, cancel: bool) {
        if self.niri.window_carry.is_none() {
            return;
        }
        if let Some(mut swipe) = self.niri.configured_swipe.take() {
            self.finish_configured_operation(&mut swipe, true);
            // Consume the remainder if Mod/Escape was released mid-stroke.
            swipe.operation = Operation::Ignored;
            swipe.binding = None;
            self.niri.configured_swipe = Some(swipe);
        }
        let duration = self
            .niri
            .window_carry
            .take()
            .unwrap()
            .binding
            .drop_duration_ms;
        self.niri.layout.carry_end_with_duration(cancel, duration);
        self.niri
            .cursor_manager
            .set_cursor_image(CursorImageStatus::default_named());
        self.niri.queue_redraw_all();
    }

    /// Called after XKB updates modifiers, before ordinary keybind dispatch.
    pub(super) fn carry_keyboard(
        &mut self,
        pressed: bool,
        raw: Option<smithay::input::keyboard::Keysym>,
        modifiers: Modifiers,
    ) -> bool {
        let Some(carry) = &self.niri.window_carry else {
            return false;
        };
        if !self.niri.layout.carry_is_active()
            || self.niri.is_locked()
            || self.niri.screenshot_ui.is_open()
            || self.is_inhibiting_shortcuts()
        {
            self.finish_window_carry(true);
            return false;
        }
        match carry_key(carry.modifiers, modifiers, pressed, raw) {
            CarryKey::Cancel => {
                self.finish_window_carry(true);
                true
            }
            CarryKey::Drop => {
                self.finish_window_carry(false);
                false
            }
            CarryKey::Consume => true,
            CarryKey::Continue => false,
        }
    }

    pub(super) fn configured_swipe_end(&mut self, cancelled: bool) -> bool {
        let Some(mut swipe) = self.niri.configured_swipe.take() else {
            return false;
        };
        self.finish_configured_operation(&mut swipe, cancelled);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niri_config::touchpad_gestures::Direction;

    fn binding(direction: Direction) -> TouchpadGesture {
        TouchpadGesture {
            fingers: 3,
            modifiers: Modifiers::SUPER,
            direction,
            threshold: 16.,
            sensitivity: 1.,
            any_modifiers: false,
            preview_scale: 0.85,
            pickup_distance: 120.,
            drop_duration_ms: 300,
            lock_axis: true,
            action: GestureAction::DragWindow,
        }
    }

    #[test]
    fn modifier_matching_is_exact_and_resolves_mod() {
        assert_eq!(
            effective_modifiers(Modifiers::COMPOSITOR, ModKey::Super),
            Modifiers::SUPER
        );
        assert_eq!(
            effective_modifiers(Modifiers::COMPOSITOR | Modifiers::SHIFT, ModKey::Super),
            Modifiers::SUPER | Modifiers::SHIFT
        );
        assert_ne!(
            effective_modifiers(Modifiers::COMPOSITOR, ModKey::Super),
            Modifiers::SUPER | Modifiers::SHIFT
        );
    }

    #[test]
    fn carry_drops_on_required_modifier_release_not_finger_up_or_shift_release() {
        use smithay::input::keyboard::Keysym;
        assert_eq!(
            carry_key(
                Modifiers::SUPER,
                Modifiers::empty(),
                false,
                Some(Keysym::Super_L)
            ),
            CarryKey::Drop
        );
        assert_eq!(
            carry_key(
                Modifiers::SUPER,
                Modifiers::SUPER,
                false,
                Some(Keysym::Shift_L)
            ),
            CarryKey::Continue
        );
        assert_eq!(
            carry_key(
                Modifiers::SUPER,
                Modifiers::SUPER,
                true,
                Some(Keysym::Escape)
            ),
            CarryKey::Cancel
        );
        assert_eq!(
            carry_key(Modifiers::SUPER, Modifiers::SUPER, true, Some(Keysym::q)),
            CarryKey::Consume
        );
        assert_eq!(
            carry_key(
                Modifiers::SUPER | Modifiers::SHIFT,
                Modifiers::SUPER,
                false,
                Some(Keysym::Shift_L)
            ),
            CarryKey::Drop
        );
    }

    #[test]
    fn exact_modifier_bindings_take_precedence_over_any() {
        let mut ignore = binding(Direction::Any);
        ignore.modifiers = Modifiers::empty();
        ignore.any_modifiers = true;
        ignore.action = GestureAction::Ignore;
        let mut plain = binding(Direction::Horizontal);
        plain.fingers = 4;
        plain.modifiers = Modifiers::empty();
        plain.action = GestureAction::ScrollView;
        let carry = TouchpadGesture {
            fingers: 4,
            action: GestureAction::CarryWindow,
            ..binding(Direction::Any)
        };
        let mut fallback4 = ignore.clone();
        fallback4.fingers = 4;
        let all = [
            ignore.clone(),
            plain.clone(),
            carry.clone(),
            fallback4.clone(),
        ];

        // Three fingers: always ignored, with or without modifiers.
        for held in [
            Modifiers::empty(),
            Modifiers::SUPER,
            Modifiers::CTRL | Modifiers::ALT,
        ] {
            assert_eq!(
                select_candidates(&all, 3, held, ModKey::Super),
                vec![ignore.clone()]
            );
        }
        // Four fingers: exact bindings first, then the "any" fallback.
        assert_eq!(
            select_candidates(&all, 4, Modifiers::empty(), ModKey::Super),
            vec![plain]
        );
        assert_eq!(
            select_candidates(&all, 4, Modifiers::SUPER, ModKey::Super),
            vec![carry]
        );
        assert_eq!(
            select_candidates(&all, 4, Modifiers::CTRL, ModKey::Super),
            vec![fallback4]
        );
        // Unbound finger counts keep niri's built-in behavior.
        assert!(select_candidates(&all, 5, Modifiers::empty(), ModKey::Super).is_empty());
    }

    #[test]
    fn later_binding_wins_a_specificity_tie() {
        let first = binding(Direction::Any);
        let mut second = first.clone();
        second.preview_scale = 0.7;
        let selected = select_binding(&[first, second], Point::from((20., 0.))).unwrap();
        assert_eq!(selected.preview_scale, 0.7);
    }

    #[test]
    fn threshold_and_direction_priority() {
        let entries = [
            binding(Direction::Any),
            binding(Direction::Horizontal),
            binding(Direction::Left),
        ];
        assert!(select_binding(&entries, Point::from((5., 0.))).is_none());
        assert_eq!(
            select_binding(&entries, Point::from((-20., 1.)))
                .unwrap()
                .direction,
            Direction::Left
        );
        assert_eq!(
            select_binding(&entries, Point::from((20., 1.)))
                .unwrap()
                .direction,
            Direction::Horizontal
        );
        assert_eq!(
            select_binding(&entries, Point::from((1., 20.)))
                .unwrap()
                .direction,
            Direction::Any
        );
    }

    fn forward(pattern: Option<&str>) -> GestureAction {
        GestureAction::ForwardToClient(
            pattern.map(|p| niri_config::utils::RegexEq(p.parse().unwrap())),
        )
    }

    #[test]
    fn forward_matches_only_the_named_app() {
        let action = forward(Some("^helium$"));
        assert!(forward_matches(&action, Some("helium")));
        assert!(!forward_matches(&action, Some("kitty")));
        // A window without an app-id never matches a pattern.
        assert!(!forward_matches(&action, None));
    }

    #[test]
    fn forward_without_a_pattern_matches_every_window() {
        let action = forward(None);
        assert!(forward_matches(&action, Some("anything")));
        assert!(forward_matches(&action, None));
    }

    #[test]
    fn other_actions_never_forward() {
        assert!(!forward_matches(&GestureAction::Ignore, Some("helium")));
        assert!(!forward_matches(&GestureAction::ScrollView, Some("helium")));
    }
}
