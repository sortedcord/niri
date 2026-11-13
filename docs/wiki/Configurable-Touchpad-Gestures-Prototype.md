# Configurable touchpad swipes (prototype)

Local patch for niri v26.04; this is not upstream configuration syntax.

```kdl
touchpad-gesture fingers=3 modifiers="Super" direction="any" threshold=4.0 sensitivity=1.0 preview-scale=0.85 pickup-distance=120.0 drop-duration-ms=300 {
    carry-window
}

touchpad-gesture fingers=4 modifiers="Mod+Shift" direction="up" {
    run { spawn "kitty"; }
}
```

For example, to move all navigation from three to four fingers and disable
three-finger swipes (and the built-in four-finger overview swipe):

```kdl
touchpad-gesture fingers=3 modifiers="any" { ignore; }
touchpad-gesture fingers=4 direction="horizontal" { scroll-view; }
touchpad-gesture fingers=4 direction="vertical" { switch-workspace; }
touchpad-gesture fingers=4 modifiers="Super" { carry-window; }
touchpad-gesture fingers=4 modifiers="any" { ignore; }
```

`touchpad-gesture` is a repeatable top-level entry:

- `fingers`: required, 3–10. Two-finger scrolling/pinching is not intercepted.
- `modifiers`: exact initial modifier combination; default `""`. Uses niri's
  modifier names, including `Mod`, `Super`, `Alt`, `Ctrl`, and `Shift`.
  `modifiers="any"` is a fallback that matches any held modifiers, used only when
  no binding for that finger count has exactly the held modifiers.
- `direction`: `any` (default), `horizontal`, `vertical`, `left`, `right`, `up`,
  or `down`, describing physical finger motion before natural scrolling.
- `threshold`: recognition distance, default 16.0; range 1.0–1000.0 unaccelerated
  libinput units.
- `sensitivity`: motion multiplier, default 1.0; range 0.1–10.0.
- `preview-scale`: fully lifted scale, default 0.85; range 0.1–1.0.
- `pickup-distance`: finger travel to complete carry pickup, default 120.0;
  range 1.0–1000.0. Larger values give a longer, more gradual lift. Independent of
  the navigation sensitivity multiplier.
- `drop-duration-ms`: carry landing duration, default 300; range 0–5000 ms.
  Larger values slow the move/grow transition; zero makes the drop instant.

Each entry contains exactly one behavior:

- `carry-window`: smoothly lift a live copy from its captured screen position
  toward center while swipes simultaneously navigate columns or workspaces
  beneath it. Cumulative finger travel drives pickup progress, smoothly shrinking
  the copy and fading it in while the actual tile remains in its original slot
  and fades toward a 25% ghost. Once fully lifted the copy stays at center. After
  a 100 ms motion pause, or on finger-up, any remaining lift completes using the
  configured window-movement animation. Resuming motion during this completion
  continues from the current progress without a backward jump. Requires a
  modifier. Lifting fingers ends only the navigation stroke; more matching-finger
  strokes can follow while the modifier remains held. Releasing any required
  modifier commits the screen-center insertion preview, then the lifted copy
  moves and grows into that slot over `drop-duration-ms`. The real tile is hidden
  during this landing to avoid duplication; the endpoint follows its actual
  animated geometry and reveals it when the copy arrives. Global/window-movement
  animation disable is honored. Escape cancels without
  removing/reinserting the original tile and returns the view to it. Dropping
  resolves a stable window-ID insertion anchor before removing the source ghost
  so column-index changes do not alter the chosen destination. Other ordinary
  key presses are consumed while carrying. Each navigation stroke locks its axis
  like native three-finger navigation. Unrelated finger counts are swallowed
  during an existing carry. The carried app retains focus.
- `drag-window`: the v1 direct-move behavior. Detach a window, shrink it, move it
  with the fingers, use edge scrolling to navigate, and drop on finger-up. There
  is no source ghost. Cancellation drops at the last valid insertion point.
- `scroll-view`: continuous horizontal scrolling, respecting natural scrolling.
- `switch-workspace`: continuous vertical workspace switching, respecting natural
  scrolling.
- `overview`: continuous overview control.
- `ignore`: claim the gesture and do nothing. Neither a built-in gesture nor the
  focused client receives it. Combine with `modifiers="any"` to disable a finger
  count entirely, or to remove leftover built-in gestures (such as the four-finger
  overview swipe, which ignores modifiers) after rebinding.
- `run { ... }`: one ordinary niri action on successful release after recognition;
  cancelled gestures do not run it.

Direction-specific bindings outrank axis bindings, then `any`; at equal
specificity last-defined wins. Entries with identical fingers/modifiers/direction
replace earlier entries, including across config includes. A matching
finger-count/modifier combination claims the entire stream. If no direction
matches, it does nothing rather than also executing a built-in gesture. With no
matching combination, niri's built-ins are unchanged.

Bindings are snapshotted at gesture start. Config reloads and extra modifiers do
not retarget an existing carry; releasing required modifiers commits it. Gestures
are blocked while locked, in screenshot/MRU UI, during other pointer grabs, and
when keyboard shortcuts are inhibited. Cancellation of an individual finger
stroke does not cancel carry; Escape, loss of the source window, or removal of
its display cleans it up.

The normal desktop does not zoom or open the overview. The mouse cursor is not
warped. Only the preview is scaled; after the gradual pickup it stays at center. The ghost is live,
not a frozen screenshot, so external application geometry changes can still
change the source tile. Carry/direct drag stay on their starting display. The
prototype does not implement pinch/rotate mappings or animated cancellation.
Pickup is gesture-driven and finishes smoothly when paused; dropping uses a
configurable timed easing. Close/output unplug clean up an in-flight landing,
and starting another pickup/native drag safely reveals the committed tile.

Design reference: julianjc84/niri `feat/configurable-touch-gestures`, reference
`464f45a`, particularly parameterized matching and continuous versus discrete
dispatch. The full touchscreen subsystem is not imported. Navigation, rendering,
insertion hints, and layout operations reuse niri v26.04's native implementation.
