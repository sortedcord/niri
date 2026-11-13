//! Carry a centered live preview while navigating beneath it. The real tile
//! remains in its original layout slot as the ghost until commit or cancellation.

use super::*;
use crate::render_helpers::offscreen::OffscreenBuffer;

const PICKUP_IDLE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub(super) struct CarryData<I> {
    pub(super) window: I,
    pub(super) output: Output,
    pub(super) scale: f64,
    source_location: Point<f64, Logical>,
    source_scale: f64,
    progress: f64,
    distance: f64,
    pickup_distance: f64,
    last_motion: Duration,
    settle: Option<Animation>,
    preview_buffer: Rc<OffscreenBuffer>,
}

impl<I> CarryData<I> {
    fn visual_progress(&self) -> f64 {
        let p = self.progress.clamp(0., 1.);
        p * p * (3. - 2. * p)
    }

    fn preview_scale(&self) -> f64 {
        self.source_scale + (self.scale - self.source_scale) * self.visual_progress()
    }
}

#[derive(Debug, Clone)]
pub(super) struct CarryDrop<I> {
    pub(super) window: I,
    pub(super) output: Output,
    from: Rectangle<f64, Logical>,
    from_opacity: f32,
    animation: Animation,
    preview_buffer: Rc<OffscreenBuffer>,
}

#[derive(Clone)]
struct Anchor<I> {
    window: I,
    before: bool,
    in_column: bool,
}

fn anchor_at<W: LayoutElement>(
    workspace: &Workspace<W>,
    position: InsertPosition,
    carried: &W::Id,
) -> Option<Anchor<W::Id>> {
    let mut tiles: Vec<_> = workspace
        .tiles_with_ipc_layouts()
        .filter_map(|(tile, layout)| {
            layout
                .pos_in_scrolling_layout
                .map(|(column, row)| (column - 1, row - 1, tile.window().id().clone()))
        })
        .collect();
    tiles.sort_by_key(|(column, row, _)| (*column, *row));
    match position {
        InsertPosition::NewColumn(index) => {
            let last_column = tiles.last()?.0;
            let column = index.min(last_column);
            // If a stacked column contains the carried window, anchor on a
            // different member so extracting it can still create a new column.
            let target = tiles
                .iter()
                .filter(|(col, _, _)| *col == column)
                .find(|(_, _, id)| id != carried)
                .or_else(|| tiles.iter().find(|(col, _, _)| *col == column))?;
            Some(Anchor {
                window: target.2.clone(),
                before: index <= last_column,
                in_column: false,
            })
        }
        InsertPosition::InColumn(column, row) => {
            let entries: Vec<_> = tiles.iter().filter(|(col, _, _)| *col == column).collect();
            let entry = entries
                .get(row)
                .copied()
                .or_else(|| entries.last().copied())?;
            Some(Anchor {
                window: entry.2.clone(),
                before: row < entries.len(),
                in_column: true,
            })
        }
        InsertPosition::Floating => None,
    }
}

fn position_for_anchor<W: LayoutElement>(
    workspace: &Workspace<W>,
    anchor: &Anchor<W::Id>,
) -> Option<InsertPosition> {
    let (_, layout) = workspace
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &anchor.window)?;
    let (column, row) = layout.pos_in_scrolling_layout?;
    let after = usize::from(!anchor.before);
    Some(if anchor.in_column {
        InsertPosition::InColumn(column - 1, row - 1 + after)
    } else {
        InsertPosition::NewColumn(column - 1 + after)
    })
}

impl<W: LayoutElement> Layout<W> {
    pub fn carry_begin(&mut self, window: &W::Id, scale: f64) -> Option<Output> {
        self.carry_begin_with_distance(window, scale, 120.)
    }

    pub fn carry_begin_with_distance(
        &mut self,
        window: &W::Id,
        scale: f64,
        pickup_distance: f64,
    ) -> Option<Output> {
        self.finish_carry_drop();
        if self.carry.is_some() || self.interactive_move.is_some() {
            return None;
        }
        let (output, source_location, source_scale) = self.monitors().find_map(|monitor| {
            for (workspace, geometry) in monitor.workspaces_with_render_geo() {
                if let Some((_, position, _)) = workspace
                    .tiles_with_render_positions()
                    .find(|(tile, _, _)| tile.window().id() == window)
                {
                    let zoom = monitor.overview_zoom();
                    return Some((
                        monitor.output().clone(),
                        geometry.loc + position.upscale(zoom),
                        zoom,
                    ));
                }
            }
            None
        })?;
        let tile = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == window)?;
        tile.set_carry_ghost(true);
        self.carry = Some(CarryData {
            window: window.clone(),
            output: output.clone(),
            scale: scale.clamp(0.1, 1.),
            source_location,
            source_scale,
            progress: 0.,
            distance: 0.,
            pickup_distance: pickup_distance.clamp(1., 1000.),
            last_motion: self.clock.now_unadjusted(),
            settle: None,
            preview_buffer: Rc::new(OffscreenBuffer::default()),
        });
        Some(output)
    }

    /// Finger movement drives the lift while the same events also pan the view.
    pub fn carry_motion(&mut self, distance: f64) {
        if !distance.is_finite() || distance <= 0. {
            return;
        }
        if let Some(carry) = &mut self.carry {
            if let Some(settle) = carry.settle.take() {
                carry.progress = carry.progress.max(settle.clamped_value());
            }
            carry.distance = carry.distance.max(carry.progress * carry.pickup_distance) + distance;
            carry.progress = (carry.distance / carry.pickup_distance).clamp(0., 1.);
            carry.last_motion = self.clock.now_unadjusted();
        }
        self.sync_carry_ghost_opacity();
    }

    pub fn carry_pickup_progress(&self) -> Option<f64> {
        self.carry.as_ref().map(|carry| carry.progress)
    }

    /// Finger-up (or a brief pause) completes the remaining lift using niri's
    /// window-movement animation, rather than leaving a half-raised window.
    pub fn carry_finish_pickup(&mut self) {
        if let Some(carry) = &mut self.carry {
            if carry.progress < 1. && carry.settle.is_none() {
                carry.settle = Some(Animation::new(
                    self.clock.clone(),
                    carry.progress,
                    1.,
                    0.,
                    self.options.animations.window_movement.0,
                ));
            }
        }
    }

    pub(super) fn advance_carry_pickup(&mut self) {
        let idle = self.carry.as_ref().is_some_and(|carry| {
            carry.progress < 1.
                && carry.settle.is_none()
                && self
                    .clock
                    .now_unadjusted()
                    .saturating_sub(carry.last_motion)
                    >= PICKUP_IDLE
        });
        if idle {
            self.carry_finish_pickup();
        }
        if let Some(carry) = &mut self.carry {
            if let Some(animation) = &carry.settle {
                carry.progress = carry.progress.max(animation.clamped_value()).clamp(0., 1.);
                if animation.is_done() || carry.progress >= 1. {
                    carry.progress = 1.;
                    carry.settle = None;
                }
            }
        }
        self.sync_carry_ghost_opacity();
    }

    pub(super) fn carry_is_animating(&self, output: Option<&Output>) -> bool {
        self.carry.as_ref().is_some_and(|carry| {
            carry.progress < 1. && output.is_none_or(|output| output == &carry.output)
        })
    }

    fn sync_carry_ghost_opacity(&mut self) {
        let Some(carry) = self.carry.clone() else {
            return;
        };
        let opacity = (1. - 0.75 * carry.visual_progress()) as f32;
        if let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &carry.window)
        {
            tile.set_carry_ghost_opacity(opacity);
        }
    }

    pub fn carry_is_active(&self) -> bool {
        self.carry.is_some()
    }

    pub fn carry_output(&self) -> Option<Output> {
        self.carry.as_ref().map(|carry| carry.output.clone())
    }

    pub(super) fn clear_carry_ghost(&mut self, window: &W::Id) {
        if let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == window)
        {
            tile.set_carry_ghost(false);
        }
    }

    pub fn carry_end(&mut self, cancel: bool) {
        self.carry_end_with_duration(cancel, 300);
    }

    pub fn carry_end_with_duration(&mut self, cancel: bool, duration_ms: u16) {
        let from = self.carry_preview_rect();
        let Some(carry) = self.carry.take() else {
            return;
        };
        self.clear_carry_ghost(&carry.window);
        if cancel
            || !self.has_window(&carry.window)
            || self.monitor_for_output(&carry.output).is_none()
        {
            if self.has_window(&carry.window) {
                self.activate_window(&carry.window);
            }
            return;
        }
        let center = output_size(&carry.output).to_point().downscale(2.);
        let is_floating = self
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&carry.window))
            .is_some_and(|(_, _, ws)| ws.is_floating(&carry.window));

        // Resolve the visible insertion preview before removing the ghost.
        // Window IDs, unlike column indices, survive the source gap closing.
        let Some((workspace_id, position, anchor)) =
            self.monitor_for_output_mut(&carry.output).map(|mon| {
                let (insert, geometry) = mon.insert_position(center);
                match insert {
                    InsertWorkspace::Existing(id) => {
                        let ws = mon.workspaces.iter().find(|ws| ws.id() == id).unwrap();
                        let position = if is_floating {
                            InsertPosition::Floating
                        } else {
                            ws.scrolling_insert_position(
                                (center - geometry.loc).downscale(mon.overview_zoom()),
                            )
                        };
                        let anchor = anchor_at(ws, position, &carry.window);
                        (id, position, anchor)
                    }
                    InsertWorkspace::NewAt(index) => {
                        let index = if mon.options.layout.empty_workspace_above_first && index == 0
                        {
                            0
                        } else if index >= mon.workspaces.len() - 1 {
                            mon.workspaces.len() - 1
                        } else {
                            mon.add_workspace_at(index);
                            index
                        };
                        (
                            mon.workspaces[index].id(),
                            if is_floating {
                                InsertPosition::Floating
                            } else {
                                InsertPosition::NewColumn(0)
                            },
                            None,
                        )
                    }
                }
            })
        else {
            return;
        };
        if anchor
            .as_ref()
            .is_some_and(|anchor| anchor.window == carry.window)
        {
            self.activate_window(&carry.window);
            self.begin_carry_drop(carry, from, duration_ms);
            return;
        }
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|ws| ws.has_window(&carry.window))
        {
            workspace.set_fullscreen(&carry.window, false);
            workspace.set_maximized(&carry.window, false);
        }
        let Some(RemovedTile {
            mut tile,
            width,
            is_full_width,
            ..
        }) = self.remove_window(&carry.window, Transaction::new())
        else {
            return;
        };
        tile.stop_move_animations();
        tile.ensure_alpha_animates_to_1();
        // Removal can delete an empty source workspace, so resolve the destination
        // by ID again instead of using a cached workspace/column index.
        let mon = self.monitor_for_output_mut(&carry.output).unwrap();
        let index = mon
            .workspaces
            .iter()
            .position(|ws| ws.id() == workspace_id)
            .unwrap_or(mon.active_workspace_idx);
        let position = anchor
            .as_ref()
            .and_then(|anchor| position_for_anchor(&mon.workspaces[index], anchor))
            .unwrap_or(position);
        match position {
            InsertPosition::InColumn(column, row) => {
                mon.add_tile_to_column(index, column, Some(row), tile, true, true);
            }
            InsertPosition::NewColumn(column) => {
                let target = MonitorAddWindowTarget::Workspace {
                    id: mon.workspaces[index].id(),
                    column_idx: Some(column),
                };
                mon.add_tile(
                    tile,
                    target,
                    ActivateWindow::Yes,
                    true,
                    width,
                    is_full_width,
                    false,
                );
            }
            InsertPosition::Floating => {
                let size = tile.window_size();
                let top_left = center - Point::from((size.w / 2., size.h / 2.));
                tile.floating_pos =
                    Some(mon.workspaces[index].floating_logical_to_size_frac(top_left));
                let target = MonitorAddWindowTarget::Workspace {
                    id: mon.workspaces[index].id(),
                    column_idx: None,
                };
                mon.add_tile(
                    tile,
                    target,
                    ActivateWindow::Yes,
                    true,
                    width,
                    is_full_width,
                    true,
                );
            }
        }
        self.focus_output(&carry.output);
        self.activate_window(&carry.window);
        self.begin_carry_drop(carry, from, duration_ms);
    }

    fn begin_carry_drop(
        &mut self,
        carry: CarryData<W::Id>,
        from: Option<Rectangle<f64, Logical>>,
        duration_ms: u16,
    ) {
        let Some(from) = from else {
            return;
        };
        if duration_ms == 0
            || self.options.animations.off
            || self.options.animations.window_movement.0.off
            || self.monitor_for_output(&carry.output).is_none()
        {
            return;
        }
        let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &carry.window)
        else {
            return;
        };
        tile.set_carry_drop_hidden(true);
        tile.alpha_animation = None;
        let pickup = carry.visual_progress();
        // Early releases must not make the window disappear when the still-
        // prominent source ghost is removed. Retain the more visible copy.
        let opacity = pickup.max(1. - 0.75 * pickup) as f32;
        self.carry_drop = Some(CarryDrop {
            window: carry.window,
            output: carry.output,
            from,
            from_opacity: opacity,
            animation: Animation::ease(
                self.clock.clone(),
                0.,
                1.,
                0.,
                u64::from(duration_ms.min(5000)),
                crate::animation::Curve::EaseOutCubic,
            ),
            preview_buffer: carry.preview_buffer,
        });
    }

    pub(super) fn finish_carry_drop(&mut self) {
        let Some(drop) = self.carry_drop.take() else {
            return;
        };
        if let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &drop.window)
        {
            tile.set_carry_drop_hidden(false);
            tile.alpha_animation = None;
        }
    }

    pub(super) fn advance_carry_drop(&mut self) {
        let done = self.carry_drop.as_ref().is_some_and(|drop| {
            drop.animation.is_done()
                || self
                    .drop_destination_rect(&drop.window, &drop.output)
                    .is_none()
        });
        if done {
            self.finish_carry_drop();
        }
    }

    pub(super) fn carry_drop_is_animating(&self, output: Option<&Output>) -> bool {
        self.carry_drop.as_ref().is_some_and(|drop| {
            !drop.animation.is_done() && output.is_none_or(|output| output == &drop.output)
        })
    }

    fn drop_destination_rect(
        &self,
        window: &W::Id,
        output: &Output,
    ) -> Option<Rectangle<f64, Logical>> {
        let monitor = self.monitor_for_output(output)?;
        let zoom = monitor.overview_zoom();
        // Unlike the visible-workspace iterator, include the destination while
        // the camera is still scrolling toward it. The endpoint follows native
        // layout/resize animations so the final handoff cannot jump.
        for (workspace, geometry) in monitor
            .workspaces
            .iter()
            .zip(monitor.workspaces_render_geo())
        {
            if let Some((tile, position, _)) = workspace
                .tiles_with_render_positions()
                .find(|(tile, _, _)| tile.window().id() == window)
            {
                return Some(Rectangle::new(
                    geometry.loc + position.upscale(zoom),
                    tile.animated_tile_size().upscale(zoom),
                ));
            }
        }
        None
    }

    pub fn carry_drop_opacity(&self) -> Option<f32> {
        let drop = self.carry_drop.as_ref()?;
        let p = drop.animation.clamped_value() as f32;
        Some(drop.from_opacity + (1. - drop.from_opacity) * p)
    }

    pub fn carry_drop_rect(&self) -> Option<Rectangle<f64, Logical>> {
        let drop = self.carry_drop.as_ref()?;
        let target = self.drop_destination_rect(&drop.window, &drop.output)?;
        let p = drop.animation.clamped_value();
        let lerp = |from: f64, to: f64| from + (to - from) * p;
        Some(Rectangle::new(
            Point::from((
                lerp(drop.from.loc.x, target.loc.x),
                lerp(drop.from.loc.y, target.loc.y),
            )),
            Size::from((
                lerp(drop.from.size.w, target.size.w),
                lerp(drop.from.size.h, target.size.h),
            )),
        ))
    }

    pub(super) fn update_carry_drop_render_elements(&mut self, output: Option<&Output>) {
        let Some(drop) = self.carry_drop.clone() else {
            return;
        };
        if output.is_some_and(|output| output != &drop.output) {
            return;
        }
        let Some(rect) = self.carry_drop_rect() else {
            return;
        };
        let view = output_size(&drop.output);
        if let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &drop.window)
        {
            let size = tile.animated_tile_size();
            let scale = Scale::from((
                rect.size.w.max(1.) / size.w.max(1.),
                rect.size.h.max(1.) / size.h.max(1.),
            ));
            tile.update_render_elements(
                true,
                Rectangle::new(rect.loc.upscale(-1.), view).downscale(scale),
            );
        }
    }

    pub(super) fn render_carry_drop_for_output<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        push: &mut dyn FnMut(RescaleRenderElement<TileRenderElement<R>>),
    ) {
        let Some(drop) = &self.carry_drop else {
            return;
        };
        if &drop.output != output {
            return;
        }
        let Some(rect) = self.carry_drop_rect() else {
            return;
        };
        let Some(tile) = self
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .find(|tile| tile.window().id() == &drop.window)
        else {
            return;
        };
        let size = tile.animated_tile_size();
        let scale = Scale::from((
            rect.size.w.max(1.) / size.w.max(1.),
            rect.size.h.max(1.) / size.h.max(1.),
        ));
        let physical = Scale::from(output.current_scale().fractional_scale());
        let opacity = self.carry_drop_opacity().unwrap_or(1.);
        let mut ctx = ctx.as_gles();
        let mut elements = Vec::new();
        tile.render_carry_preview(
            ctx.r(),
            Point::from((0., 0.)),
            XrayPos::new(rect.loc, scale.x),
            &mut |element| elements.push(element),
        );
        match drop
            .preview_buffer
            .render(ctx.renderer, physical, &elements)
        {
            Ok((element, _, data)) => {
                let offset = element.offset();
                tile.window().set_offscreen_data(Some(data));
                let element: TileRenderElement<R> = element
                    .with_alpha(opacity)
                    .with_offset(rect.loc + offset)
                    .into();
                push(RescaleRenderElement::from_element(
                    element,
                    rect.loc.to_physical_precise_round(physical),
                    scale,
                ));
            }
            Err(error) => warn!("error rendering carry landing: {error:?}"),
        }
    }

    pub(super) fn update_carry_insert_hint(&mut self, output: Option<&Output>) {
        let Some(carry) = self.carry.clone() else {
            return;
        };
        if output.is_some_and(|output| output != &carry.output) {
            return;
        }
        let floating = self
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&carry.window))
            .is_some_and(|(_, _, ws)| ws.is_floating(&carry.window));
        let Some(mon) = self.monitor_for_output_mut(&carry.output) else {
            return;
        };
        let center = output_size(&carry.output).to_point().downscale(2.);
        let (workspace, geometry) = mon.insert_position(center);
        let position = if floating {
            InsertPosition::Floating
        } else {
            match workspace {
                InsertWorkspace::Existing(id) => {
                    let ws = mon.workspaces.iter().find(|ws| ws.id() == id).unwrap();
                    ws.scrolling_insert_position(
                        (center - geometry.loc).downscale(mon.overview_zoom()),
                    )
                }
                InsertWorkspace::NewAt(_) => InsertPosition::NewColumn(0),
            }
        };
        mon.insert_hint = Some(InsertHint {
            workspace,
            position,
            corner_radius: CornerRadius::default(),
        });
    }

    pub fn carry_preview_rect(&self) -> Option<Rectangle<f64, Logical>> {
        let carry = self.carry.as_ref()?;
        let tile = self
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .find(|tile| tile.window().id() == &carry.window)?;
        let view = output_size(&carry.output);
        let size = tile.animated_tile_size().upscale(carry.preview_scale());
        let center_location = Point::from(((view.w - size.w) / 2., (view.h - size.h) / 2.));
        let p = carry.visual_progress();
        let location = carry.source_location.upscale(1. - p) + center_location.upscale(p);
        Some(Rectangle::new(location, size))
    }

    pub(super) fn update_carry_render_elements(&mut self, output: Option<&Output>) {
        let Some(carry) = self.carry.clone() else {
            return;
        };
        if output.is_some_and(|output| output != &carry.output) {
            return;
        }
        let view = output_size(&carry.output);
        let Some(preview) = self.carry_preview_rect() else {
            return;
        };
        if let Some(tile) = self
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &carry.window)
        {
            tile.update_render_elements(
                true,
                Rectangle::new(preview.loc.upscale(-1.), view).downscale(carry.preview_scale()),
            );
        }
    }

    pub(super) fn render_carry_for_output<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        push: &mut dyn FnMut(RescaleRenderElement<TileRenderElement<R>>),
    ) -> bool {
        let Some(carry) = &self.carry else {
            return false;
        };
        if &carry.output != output {
            return false;
        }
        let Some(tile) = self
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .find(|tile| tile.window().id() == &carry.window)
        else {
            return false;
        };
        let Some(preview) = self.carry_preview_rect() else {
            return false;
        };
        let location = preview.loc;
        let opacity = carry.visual_progress() as f32;
        if opacity <= 0. {
            return true;
        }
        let physical_scale = Scale::from(output.current_scale().fractional_scale());
        let preview_scale = carry.preview_scale();
        let mut ctx = ctx.as_gles();
        let mut elements = Vec::new();
        tile.render_carry_preview(
            ctx.r(),
            Point::from((0., 0.)),
            XrayPos::new(location, preview_scale),
            &mut |element| elements.push(element),
        );
        match carry
            .preview_buffer
            .render(ctx.renderer, physical_scale, &elements)
        {
            Ok((element, _, data)) => {
                let offset = element.offset();
                tile.window().set_offscreen_data(Some(data));
                let element: TileRenderElement<R> = element
                    .with_alpha(opacity)
                    .with_offset(location + offset)
                    .into();
                push(RescaleRenderElement::from_element(
                    element,
                    location.to_physical_precise_round(physical_scale),
                    preview_scale,
                ));
            }
            Err(error) => warn!("error rendering progressive carry preview: {error:?}"),
        }
        true
    }
}
