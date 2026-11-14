//! Column scrolling and workspace switching driven together by one touchpad swipe stroke.

use smithay::output::Output;
use smithay::utils::{Logical, Point};

use super::{Layout, LayoutElement};
use std::time::Duration;

/// Navigation state of one swipe stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Navigation {
    lock_axis: bool,
    /// Locked mode: the axis chosen from the first motion (`true` = horizontal).
    axis: Option<bool>,
    /// Unlocked mode: whether the workspace switch gesture has started.
    switching: bool,
    /// Unlocked mode: workspace whose columns horizontal motion currently scrolls.
    view_workspace: Option<usize>,
}

impl Navigation {
    pub fn new(lock_axis: bool) -> Self {
        Self {
            lock_axis,
            axis: None,
            switching: false,
            view_workspace: None,
        }
    }
}

impl<W: LayoutElement> Layout<W> {
    /// Feeds one stroke's motion (already natural-scroll adjusted) into column scrolling and/or
    /// workspace switching on `output`. Returns whether any navigation is ongoing.
    ///
    /// Locked strokes follow only the axis of their first motion, like niri's built-in swipes.
    /// Unlocked strokes switch workspaces with vertical motion while horizontal motion scrolls
    /// the columns of whichever workspace is nearest the center.
    pub fn navigation_update(
        &mut self,
        nav: &mut Navigation,
        output: &Output,
        delta: Point<f64, Logical>,
        timestamp: Duration,
    ) -> bool {
        if nav.lock_axis {
            let horizontal = match nav.axis {
                Some(horizontal) => horizontal,
                None => {
                    let horizontal = delta.x.abs() > delta.y.abs();
                    if horizontal {
                        self.view_offset_gesture_begin(output, None, true);
                    } else {
                        self.workspace_switch_gesture_begin(output, true);
                    }
                    nav.axis = Some(horizontal);
                    horizontal
                }
            };
            return if horizontal {
                self.view_offset_gesture_update(delta.x, timestamp, true)
                    .is_some()
            } else {
                self.workspace_switch_gesture_update(delta.y, timestamp, true)
                    .is_some()
            };
        }

        if !nav.switching {
            self.workspace_switch_gesture_begin(output, true);
            nav.switching = true;
        }
        let switched = self
            .workspace_switch_gesture_update(delta.y, timestamp, true)
            .is_some();

        let nearest = self.nearest_workspace_idx(output);
        if nearest.is_some() && nearest != nav.view_workspace {
            // Hand horizontal scrolling over to the workspace now in view; this also lets the
            // previous one settle onto a column.
            self.view_offset_gesture_begin(output, nearest, true);
            nav.view_workspace = nearest;
        }
        let scrolled = self
            .view_offset_gesture_update(delta.x, timestamp, true)
            .is_some();

        switched || scrolled
    }

    /// Ends one stroke, letting columns and workspaces settle with the stroke's momentum.
    pub fn navigation_end(&mut self, nav: &Navigation) {
        if nav.lock_axis {
            match nav.axis {
                Some(true) => {
                    self.view_offset_gesture_end(Some(true));
                }
                Some(false) => {
                    self.workspace_switch_gesture_end(Some(true));
                }
                None => (),
            }
        } else {
            // Settle columns first, while their workspace is still the one being scrolled.
            if nav.view_workspace.is_some() {
                self.view_offset_gesture_end(Some(true));
            }
            if nav.switching {
                self.workspace_switch_gesture_end(Some(true));
            }
        }
    }
}
