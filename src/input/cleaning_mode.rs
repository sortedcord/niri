//! Input handling for cleaning mode.
//!
//! While cleaning mode is active, every input event is dropped except the ones that keep the
//! session safe and let the user leave:
//! - devices being added and removed, so a keyboard plugged in mid-clean still works;
//! - the lid switch, so closing the laptop still suspends it;
//! - key presses, which are only used to match the "clear" unlock sequence and are never sent to
//!   clients or bound to anything (including VT switching and the power key).

use smithay::backend::input::{Event, InputEvent, KeyState, KeyboardKeyEvent, Keycode};
use smithay::input::keyboard::FilterResult;
use smithay::utils::SERIAL_COUNTER;

use super::backend_ext::NiriInputBackend as InputBackend;
use crate::niri::State;
use crate::ui::cleaning_mode::KeyOutcome;

impl State {
    /// Handles an input event while cleaning mode is active.
    pub(super) fn process_cleaning_mode_event<I: InputBackend + 'static>(
        &mut self,
        event: InputEvent<I>,
    ) where
        I::Device: 'static,
    {
        use InputEvent::*;
        match event {
            DeviceAdded { device } => self.on_device_added(device),
            DeviceRemoved { device } => self.on_device_removed(device),
            SwitchToggle { event } => self.on_switch_toggle::<I>(event),
            Keyboard { event } => {
                self.cleaning_mode_key(event.key_code(), event.state(), Event::time_msec(&event));
            }
            // Everything else (pointer, touch, tablet, gestures, scrolling) is ignored.
            _ => (),
        }
    }

    /// Feeds a key event to the unlock sequence. The key never reaches a client.
    pub fn cleaning_mode_key(&mut self, key_code: Keycode, state: KeyState, time: u32) {
        // Still run the key through XKB, so that modifier and lock state stay correct for when
        // cleaning mode ends. Intercepting keeps it away from clients.
        let mut sym = None;
        let serial = SERIAL_COUNTER.next_serial();
        self.niri.seat.get_keyboard().unwrap().input(
            self,
            key_code,
            state,
            serial,
            time,
            |_, _, keysym| {
                // The key's own letter, without modifiers, so Shift or Caps Lock don't matter.
                sym = keysym.raw_latin_sym_or_raw_current_sym();
                FilterResult::<()>::Intercept(())
            },
        );

        match self.niri.cleaning_mode.on_key(state, sym) {
            KeyOutcome::Ignored => (),
            KeyOutcome::Advanced | KeyOutcome::Reset => self.niri.queue_redraw_all(),
            KeyOutcome::Complete => self.exit_cleaning_mode(),
        }
    }

    /// Starts ignoring all input until "clear" is typed.
    pub fn enter_cleaning_mode(&mut self) {
        if !self.niri.cleaning_mode.enter() {
            return;
        }
        info!("entering cleaning mode");

        // Anything in progress would never see its end events.
        self.cancel_interactions_for_cleaning_mode();

        // Don't leave transient UI that would normally take the next key press.
        self.niri.hotkey_overlay.hide();
        self.niri.exit_confirm_dialog.hide();

        if let Some(token) = self.niri.bind_repeat_timer.take() {
            self.niri.event_loop.remove(token);
        }
        // Releases of keys that started a bind would otherwise stay swallowed after leaving.
        self.niri.suppressed_keys.clear();

        // The overlay is how the user learns how to leave.
        self.niri.activate_monitors(&mut self.backend);

        self.update_keyboard_focus();
        self.refresh_pointer_contents();
        self.niri.refresh_idle_inhibit();
        self.niri.queue_redraw_all();
    }

    /// Goes back to normal input.
    pub fn exit_cleaning_mode(&mut self) {
        if !self.niri.cleaning_mode.exit() {
            return;
        }
        info!("leaving cleaning mode");

        // Keys still held, such as the last letter of "clear", were never sent to a client, so
        // their releases must not be either.
        let held = self.niri.seat.get_keyboard().unwrap().pressed_keys();
        self.niri.suppressed_keys.clear();
        self.niri.suppressed_keys.extend(held);

        self.update_keyboard_focus();
        self.refresh_pointer_contents();
        self.niri.refresh_idle_inhibit();
        // Typing "clear" was activity, and the idle timer should not count the time spent
        // cleaning.
        self.niri.notify_activity();
        self.niri.queue_redraw_all();
    }

    fn cancel_interactions_for_cleaning_mode(&mut self) {
        self.cancel_configured_gestures();

        // Same cleanup as the end of a built-in swipe.
        self.niri.gesture_swipe_3f_cumulative = None;
        self.niri.layout.workspace_switch_gesture_end(Some(true));
        self.niri.layout.view_offset_gesture_end(Some(true));
        self.niri.layout.overview_gesture_end();

        // Interactive moves and resizes, drag and drop, and so on.
        self.niri.seat.get_pointer().unwrap().unset_grab(
            self,
            SERIAL_COUNTER.next_serial(),
            crate::utils::get_monotonic_time().as_millis() as u32,
        );
        if let Some(touch) = self.niri.seat.get_touch() {
            touch.unset_grab(self);
        }
    }
}
