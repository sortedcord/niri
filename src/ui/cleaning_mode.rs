//! Cleaning mode: all input is ignored until "clear" is typed.
//!
//! This lets the keyboard, touchpad and other peripherals be wiped down without switching them
//! off. The input gating lives in `input/mod.rs`; this module holds the state, the key sequence
//! matching and the overlay that explains how to leave.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

use ordered_float::NotNan;
use pangocairo::cairo::{self, ImageSurface};
use pangocairo::pango::{Alignment, FontDescription};
use smithay::backend::input::KeyState;
use smithay::backend::renderer::element::Kind;
use smithay::input::keyboard::Keysym;
use smithay::output::Output;
use smithay::reexports::gbm::Format as Fourcc;
use smithay::utils::{Point, Transform};

use crate::niri_render_elements;
use crate::render_helpers::memory::MemoryBuffer;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::utils::{output_size, to_physical_precise_round};

/// Keys to type, in order, to leave cleaning mode.
pub const UNLOCK_KEYS: [Keysym; 5] = [Keysym::c, Keysym::l, Keysym::e, Keysym::a, Keysym::r];

const PADDING: i32 = 24;
const FONT: &str = "sans 16px";
/// The whole screen goes black, so only the prompt is visible.
const BACKDROP_COLOR: [f32; 4] = [0., 0., 0., 1.];

/// What a key event did to the unlock sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The event did not affect the sequence (a release, or a modifier).
    Ignored,
    /// The sequence advanced, but is not complete.
    Advanced,
    /// A wrong key: the sequence started over.
    Reset,
    /// The last key of the sequence was typed.
    Complete,
}

pub struct CleaningMode {
    active: bool,
    /// How many keys of [`UNLOCK_KEYS`] have been typed in a row.
    progress: usize,
    buffers: RefCell<HashMap<(NotNan<f64>, usize), Option<MemoryBuffer>>>,
}

niri_render_elements! {
    CleaningModeRenderElement => {
        Texture = PrimaryGpuTextureRenderElement,
        SolidColor = SolidColorRenderElement,
    }
}

struct OutputData {
    backdrop: SolidColorBuffer,
}

impl Default for CleaningMode {
    fn default() -> Self {
        Self::new()
    }
}

impl CleaningMode {
    pub fn new() -> Self {
        Self {
            active: false,
            progress: 0,
            buffers: RefCell::new(HashMap::new()),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// How many unlock keys have been typed in a row.
    pub fn progress(&self) -> usize {
        self.progress
    }

    /// Starts cleaning mode. Returns false if it was already active.
    pub fn enter(&mut self) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.progress = 0;
        true
    }

    /// Ends cleaning mode. Returns false if it was not active.
    pub fn exit(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.progress = 0;
        true
    }

    /// Feeds one key event, identified by the key's unmodified Latin keysym, into the unlock
    /// sequence.
    ///
    /// Modifiers and releases are ignored, so holding Shift or letting go of a key does not
    /// disturb the sequence. Any other key that is not the next one starts it over, and counts as
    /// the first key if it happens to be "c".
    pub fn on_key(&mut self, state: KeyState, sym: Option<Keysym>) -> KeyOutcome {
        if !self.active || state != KeyState::Pressed {
            return KeyOutcome::Ignored;
        }

        // A completed sequence should be followed by leaving; if it was not, start over.
        if self.progress >= UNLOCK_KEYS.len() {
            self.progress = 0;
        }

        let Some(sym) = sym else {
            return self.restart(None);
        };
        if is_modifier(sym) {
            return KeyOutcome::Ignored;
        }

        if sym == UNLOCK_KEYS[self.progress] {
            self.progress += 1;
            if self.progress == UNLOCK_KEYS.len() {
                return KeyOutcome::Complete;
            }
            return KeyOutcome::Advanced;
        }

        self.restart(Some(sym))
    }

    fn restart(&mut self, sym: Option<Keysym>) -> KeyOutcome {
        let was = self.progress;
        self.progress = usize::from(sym == Some(UNLOCK_KEYS[0]));
        match (was, self.progress) {
            // A "c" can begin a new attempt.
            (_, 1) => KeyOutcome::Advanced,
            // Nothing was typed and nothing is now, so there is nothing to redraw.
            (0, 0) => KeyOutcome::Ignored,
            _ => KeyOutcome::Reset,
        }
    }

    pub fn render<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        output: &Output,
        push: &mut dyn FnMut(CleaningModeRenderElement),
    ) {
        if !self.active {
            return;
        }
        let _span = tracy_client::span!("CleaningMode::render");

        let scale = output.current_scale().fractional_scale();
        let output_size = output_size(output);

        let mut buffers = self.buffers.borrow_mut();
        // Keep only the buffers that can still be used: other progress values are re-rendered on
        // demand, so this stays at one buffer per scale.
        buffers.retain(|(_, progress), _| *progress == self.progress);
        let buffer = buffers
            .entry((NotNan::new(scale).unwrap(), self.progress))
            .or_insert_with(|| match render(scale, self.progress) {
                Ok(buffer) => Some(buffer),
                Err(err) => {
                    warn!("error rendering the cleaning mode overlay: {err:?}");
                    None
                }
            });

        if let Some(buffer) = buffer {
            let size = buffer.logical_size();
            if let Ok(texture) =
                TextureBuffer::from_memory_buffer(renderer.as_gles_renderer(), buffer)
            {
                let location = (output_size.to_point() - size.to_point()).downscale(2.);
                let mut location = location.to_physical_precise_round(scale).to_logical(scale);
                location.x = f64::max(0., location.x);
                location.y = f64::max(0., location.y);

                let elem = TextureRenderElement::from_texture_buffer(
                    texture,
                    location,
                    1.,
                    None,
                    None,
                    Kind::Unspecified,
                );
                push(CleaningModeRenderElement::Texture(
                    PrimaryGpuTextureRenderElement(elem),
                ));
            }
        }

        // The backdrop covers everything, whether or not the text could be drawn.
        let data = output.user_data().get_or_insert(|| {
            Mutex::new(OutputData {
                backdrop: SolidColorBuffer::new(output_size, BACKDROP_COLOR),
            })
        });
        let mut data = data.lock().unwrap();
        data.backdrop.resize(output_size);

        let elem = SolidColorRenderElement::from_buffer(
            &data.backdrop,
            Point::new(0., 0.),
            1.,
            Kind::Unspecified,
        );
        push(CleaningModeRenderElement::SolidColor(elem));
    }
}

/// Whether the keysym is a modifier, which never affects the unlock sequence.
fn is_modifier(sym: Keysym) -> bool {
    matches!(
        sym,
        Keysym::Shift_L
            | Keysym::Shift_R
            | Keysym::Control_L
            | Keysym::Control_R
            | Keysym::Alt_L
            | Keysym::Alt_R
            | Keysym::Meta_L
            | Keysym::Meta_R
            | Keysym::Super_L
            | Keysym::Super_R
            | Keysym::Hyper_L
            | Keysym::Hyper_R
            | Keysym::Caps_Lock
            | Keysym::Shift_Lock
            | Keysym::Num_Lock
            | Keysym::Scroll_Lock
            | Keysym::ISO_Level3_Shift
            | Keysym::ISO_Level5_Shift
            | Keysym::Mode_switch
    )
}

fn markup(progress: usize) -> String {
    const TYPED: &str = "#6EE787";
    const PENDING: &str = "#FFFFFF";

    let letters: Vec<String> = ["c", "l", "e", "a", "r"]
        .iter()
        .enumerate()
        .map(|(i, letter)| {
            let (color, bg) = if i < progress {
                (TYPED, "#1F4D2B")
            } else {
                (PENDING, "#2C2C2C")
            };
            format!("<span face='mono' foreground='{color}' bgcolor='{bg}'> {letter} </span>")
        })
        .collect();

    format!(
        "<b>Cleaning mode</b>\n\n\
         The keyboard, touchpad and mouse are turned off.\n\
         Type {} to exit.",
        letters.join(" ")
    )
}

fn render(scale: f64, progress: usize) -> anyhow::Result<MemoryBuffer> {
    let _span = tracy_client::span!("cleaning_mode::render");

    let markup = markup(progress);

    let padding: i32 = to_physical_precise_round(scale, PADDING);

    let mut font = FontDescription::from_string(FONT);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));

    let surface = ImageSurface::create(cairo::Format::ARgb32, 0, 0)?;
    let cr = cairo::Context::new(&surface)?;
    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    layout.set_font_description(Some(&font));
    layout.set_alignment(Alignment::Center);
    layout.set_markup(&markup);

    let (mut width, mut height) = layout.pixel_size();
    width += padding * 2;
    height += padding * 2;

    // Black, like the backdrop, so the text appears to float on the screen.
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;
    cr.set_source_rgb(0., 0., 0.);
    cr.paint()?;

    cr.move_to(padding.into(), padding.into());
    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    layout.set_font_description(Some(&font));
    layout.set_alignment(Alignment::Center);
    layout.set_markup(&markup);

    cr.set_source_rgb(1., 1., 1.);
    pangocairo::functions::show_layout(&cr, &layout);
    drop(cr);

    let data = surface.take_data().unwrap();
    let buffer = MemoryBuffer::new(
        data.to_vec(),
        Fourcc::Argb8888,
        (width, height),
        scale,
        Transform::Normal,
    );

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active() -> CleaningMode {
        let mut mode = CleaningMode::new();
        assert!(mode.enter());
        mode
    }

    fn type_keys(mode: &mut CleaningMode, keys: &[Keysym]) -> Vec<KeyOutcome> {
        keys.iter()
            .map(|key| mode.on_key(KeyState::Pressed, Some(*key)))
            .collect()
    }

    #[test]
    fn enter_and_exit_are_idempotent() {
        let mut mode = CleaningMode::new();
        assert!(!mode.is_active());
        assert!(!mode.exit());
        assert!(mode.enter());
        assert!(!mode.enter());
        assert!(mode.is_active());
        assert!(mode.exit());
        assert!(!mode.exit());
        assert!(!mode.is_active());
    }

    #[test]
    fn typing_clear_completes_the_sequence() {
        let mut mode = active();
        assert_eq!(
            type_keys(&mut mode, &UNLOCK_KEYS),
            vec![
                KeyOutcome::Advanced,
                KeyOutcome::Advanced,
                KeyOutcome::Advanced,
                KeyOutcome::Advanced,
                KeyOutcome::Complete,
            ]
        );
    }

    #[test]
    fn a_wrong_key_starts_the_sequence_over() {
        let mut mode = active();
        type_keys(&mut mode, &[Keysym::c, Keysym::l, Keysym::e]);
        assert_eq!(mode.progress(), 3);
        // "x" breaks it.
        assert_eq!(
            mode.on_key(KeyState::Pressed, Some(Keysym::x)),
            KeyOutcome::Reset
        );
        assert_eq!(mode.progress(), 0);
        // "a" and "r" alone no longer finish it.
        type_keys(&mut mode, &[Keysym::a, Keysym::r]);
        assert_eq!(mode.progress(), 0);
        assert_eq!(
            type_keys(&mut mode, &UNLOCK_KEYS).pop(),
            Some(KeyOutcome::Complete)
        );
    }

    #[test]
    fn a_wrong_c_counts_as_the_first_key() {
        let mut mode = active();
        type_keys(&mut mode, &[Keysym::c, Keysym::l]);
        // "c" is not "e", but it can begin a new attempt: "c" "c" "l" "e" "a" "r".
        assert_eq!(
            mode.on_key(KeyState::Pressed, Some(Keysym::c)),
            KeyOutcome::Advanced
        );
        assert_eq!(mode.progress(), 1);
        let outcomes = type_keys(&mut mode, &UNLOCK_KEYS[1..]);
        assert_eq!(outcomes.last(), Some(&KeyOutcome::Complete));
    }

    #[test]
    fn modifiers_and_releases_do_not_disturb_the_sequence() {
        let mut mode = active();
        type_keys(&mut mode, &[Keysym::c, Keysym::l]);
        for modifier in [
            Keysym::Shift_L,
            Keysym::Control_R,
            Keysym::Caps_Lock,
            Keysym::Super_L,
        ] {
            assert_eq!(
                mode.on_key(KeyState::Pressed, Some(modifier)),
                KeyOutcome::Ignored
            );
        }
        assert_eq!(
            mode.on_key(KeyState::Released, Some(Keysym::x)),
            KeyOutcome::Ignored
        );
        assert_eq!(mode.progress(), 2);
        let outcomes = type_keys(&mut mode, &UNLOCK_KEYS[2..]);
        assert_eq!(outcomes.last(), Some(&KeyOutcome::Complete));
    }

    #[test]
    fn keys_without_a_keysym_reset_the_sequence() {
        let mut mode = active();
        type_keys(&mut mode, &[Keysym::c, Keysym::l]);
        assert_eq!(mode.on_key(KeyState::Pressed, None), KeyOutcome::Reset);
        assert_eq!(mode.progress(), 0);
    }

    #[test]
    fn stray_keys_at_the_start_change_nothing() {
        let mut mode = active();
        assert_eq!(
            mode.on_key(KeyState::Pressed, Some(Keysym::q)),
            KeyOutcome::Ignored
        );
        assert_eq!(mode.progress(), 0);
    }

    #[test]
    fn inactive_mode_ignores_keys() {
        let mut mode = CleaningMode::new();
        assert_eq!(
            mode.on_key(KeyState::Pressed, Some(Keysym::c)),
            KeyOutcome::Ignored
        );
        assert_eq!(mode.progress(), 0);
    }

    #[test]
    fn entering_again_starts_from_zero() {
        let mut mode = active();
        type_keys(&mut mode, &[Keysym::c, Keysym::l]);
        mode.exit();
        assert!(mode.enter());
        assert_eq!(mode.progress(), 0);
    }

    #[test]
    fn overlay_renders_and_shows_progress() {
        let none = render(1., 0).unwrap();
        let some = render(1., 3).unwrap();
        // Same text, so the same size, but the typed letters are drawn differently.
        assert_eq!(none.logical_size(), some.logical_size());
        assert!(none.logical_size().w > 100. && none.logical_size().h > 50.);
        // The prompt has no panel: its corners are opaque black, same as the backdrop. (ARGB8888
        // is stored as B, G, R, A bytes.)
        let w = none.size().w as usize;
        let h = none.size().h as usize;
        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * w + x) * 4;
            none.data()[i..i + 4].try_into().unwrap()
        };
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert_eq!(pixel(x, y), [0, 0, 0, 255], "corner at ({x}, {y})");
        }
        assert_eq!(BACKDROP_COLOR, [0., 0., 0., 1.]);
        assert_ne!(none.data(), some.data());
        // A higher scale gives more pixels for the same logical size.
        let scaled = render(2., 0).unwrap();
        assert!(scaled.logical_size().w > 100.);
    }

    #[test]
    fn overlay_text_highlights_typed_letters() {
        let none = markup(0);
        let some = markup(3);
        assert!(none.contains("Cleaning mode"));
        assert_ne!(none, some);
        assert_eq!(some.matches("#6EE787").count(), 3);
        assert_eq!(none.matches("#6EE787").count(), 0);
    }
}
