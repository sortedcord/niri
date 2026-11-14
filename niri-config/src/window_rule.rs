use niri_ipc::ColumnDisplay;

use crate::appearance::{
    BackgroundEffect, BackgroundEffectRule, BlockOutFrom, BorderRule, CornerRadius, ShadowRule,
    TabIndicatorRule,
};
use crate::layout::DefaultPresetSize;
use crate::utils::{MergeWith, RegexEq};
use crate::FloatOrInt;

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowRule {
    #[knuffel(children(name = "match"))]
    pub matches: Vec<Match>,
    #[knuffel(children(name = "exclude"))]
    pub excludes: Vec<Match>,

    // Rules applied at initial configure.
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child)]
    pub default_window_height: Option<DefaultPresetSize>,
    #[knuffel(child, unwrap(argument))]
    pub open_on_output: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_on_workspace: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized_to_edges: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_fullscreen: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_floating: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_focused: Option<bool>,

    // Rules applied dynamically.
    #[knuffel(child, unwrap(argument))]
    pub min_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub min_height: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_height: Option<u16>,

    #[knuffel(child, default)]
    pub focus_ring: BorderRule,
    #[knuffel(child, default)]
    pub border: BorderRule,
    #[knuffel(child, default)]
    pub shadow: ShadowRule,
    #[knuffel(child, default)]
    pub tab_indicator: TabIndicatorRule,
    #[knuffel(child, unwrap(argument))]
    pub draw_border_with_background: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, unwrap(argument))]
    pub clip_to_geometry: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub baba_is_float: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub block_out_from: Option<BlockOutFrom>,
    #[knuffel(child, unwrap(argument))]
    pub variable_refresh_rate: Option<bool>,
    #[knuffel(child, unwrap(argument, str))]
    pub default_column_display: Option<ColumnDisplay>,
    #[knuffel(child)]
    pub default_floating_position: Option<FloatingPosition>,
    #[knuffel(child)]
    pub float_spawn: Option<FloatSpawn>,
    #[knuffel(child, unwrap(argument))]
    pub scroll_factor: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub tiled_state: Option<bool>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
    #[knuffel(child, default)]
    pub popups: PopupsRule,
}

/// Rules for popup surfaces.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct PopupsRule {
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
}

/// Resolved popup-specific rules.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ResolvedPopupsRules {
    /// Extra opacity to draw popups with.
    pub opacity: Option<f32>,

    /// Corner radius to assume the popups have.
    pub geometry_corner_radius: Option<CornerRadius>,

    /// Background effect configuration for popups.
    pub background_effect: BackgroundEffect,
}

impl MergeWith<PopupsRule> for ResolvedPopupsRules {
    fn merge_with(&mut self, part: &PopupsRule) {
        if let Some(x) = part.opacity {
            self.opacity = Some(x);
        }
        if let Some(x) = part.geometry_corner_radius {
            self.geometry_corner_radius = Some(x);
        }
        self.background_effect.merge_with(&part.background_effect);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Match {
    #[knuffel(property, str)]
    pub app_id: Option<RegexEq>,
    #[knuffel(property, str)]
    pub title: Option<RegexEq>,
    #[knuffel(property)]
    pub is_active: Option<bool>,
    #[knuffel(property)]
    pub is_focused: Option<bool>,
    #[knuffel(property)]
    pub is_active_in_column: Option<bool>,
    #[knuffel(property)]
    pub is_floating: Option<bool>,
    #[knuffel(property)]
    pub is_window_cast_target: Option<bool>,
    #[knuffel(property)]
    pub is_urgent: Option<bool>,
    #[knuffel(property)]
    pub at_startup: Option<bool>,
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct FloatingPosition {
    #[knuffel(property)]
    pub x: FloatOrInt<-65535, 65535>,
    #[knuffel(property)]
    pub y: FloatOrInt<-65535, 65535>,
    #[knuffel(property, default)]
    pub relative_to: RelativeTo,
}

/// Where a window that opens floating appears, relative to the mouse cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatSpawn {
    pub mode: SpawnMode,
    /// Window size requested for the window, in logical pixels, without borders.
    pub width: Option<u16>,
    pub height: Option<u16>,
    /// Logical pixels between the cursor and the window, for `smart-edge` only.
    pub distance: f64,
}

#[derive(knuffel::DecodeScalar, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// The window's center is at the cursor.
    Centered,
    /// The window's corner or edge is at the cursor, chosen by where the cursor is on screen.
    SmartEdge,
}

#[derive(knuffel::Decode)]
struct RawFloatSpawn {
    #[knuffel(property)]
    mode: SpawnMode,
    #[knuffel(property)]
    width: Option<u16>,
    #[knuffel(property)]
    height: Option<u16>,
    #[knuffel(property)]
    distance: Option<FloatOrInt<0, 65535>>,
}

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for FloatSpawn {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, knuffel::errors::DecodeError<S>> {
        let raw = RawFloatSpawn::decode_node(node, ctx)?;
        let error =
            |message: &str| knuffel::errors::DecodeError::unexpected(node, "float-spawn", message);

        if raw.width == Some(0) || raw.height == Some(0) {
            return Err(error("width and height must be positive"));
        }

        match raw.mode {
            SpawnMode::Centered => {
                if raw.distance.is_some() {
                    return Err(error("distance only applies to mode=\"smart-edge\""));
                }
            }
            SpawnMode::SmartEdge => {
                if raw.width.is_none() || raw.height.is_none() {
                    return Err(error("mode=\"smart-edge\" needs both width and height"));
                }
            }
        }

        Ok(Self {
            mode: raw.mode,
            width: raw.width,
            height: raw.height,
            distance: raw.distance.map_or(0., |distance| distance.0),
        })
    }
}

#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTo {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

#[cfg(test)]
mod float_spawn_tests {
    use super::*;
    use crate::Config;

    fn parse(rule: &str) -> Result<Option<FloatSpawn>, miette::Report> {
        let text = format!("window-rule {{\n{rule}\n}}");
        let config = Config::parse_mem(&text).map_err(miette::Report::new)?;
        Ok(config.window_rules[0].float_spawn)
    }

    #[test]
    fn parses_smart_edge_with_size_and_distance() {
        let spawn = parse(r#"float-spawn mode="smart-edge" width=400 height=300 distance=12"#)
            .unwrap()
            .unwrap();
        assert_eq!(
            spawn,
            FloatSpawn {
                mode: SpawnMode::SmartEdge,
                width: Some(400),
                height: Some(300),
                distance: 12.,
            }
        );
    }

    #[test]
    fn centered_needs_no_size_and_distance_defaults_to_zero() {
        let spawn = parse(r#"float-spawn mode="centered""#).unwrap().unwrap();
        assert_eq!(spawn.mode, SpawnMode::Centered);
        assert_eq!(
            (spawn.width, spawn.height, spawn.distance),
            (None, None, 0.)
        );
    }

    #[test]
    fn rejects_invalid_float_spawn() {
        for rule in [
            // smart-edge places by the window's size, so it needs one.
            r#"float-spawn mode="smart-edge" width=400"#,
            r#"float-spawn mode="smart-edge""#,
            // Centered placement has no distance to apply.
            r#"float-spawn mode="centered" distance=10"#,
            r#"float-spawn mode="centered" width=0 height=100"#,
            r#"float-spawn mode="diagonal""#,
            r#"float-spawn"#,
        ] {
            assert!(parse(rule).is_err(), "should reject: {rule}");
        }
    }
}
