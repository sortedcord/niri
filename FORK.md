# Configurable touchpad carry gestures

This feature branch contains the complete, manually tested gesture prototype,
based on upstream **niri v26.04**, commit
`8ed0da44d974c32c6877d2f4630c314da0717ecb`.

**Branch:** `feat/configurable-touchpad-carry-v26.04`

It is not rebased onto current upstream main and is not an official niri release.
The compositor implementation is the tested prototype, unchanged; this fork adds
repository-level instructions and example configurations. Smart-border helpers
and personal desktop configuration are not included.

## What it adds

- Configurable touchpad swipe bindings by finger count, modifiers, and direction.
- Continuous view/workspace/overview navigation or one-shot normal niri actions.
- Optional direct window dragging, or the centered **carry-window** interaction.
- Gesture-driven pickup progress, smooth completion after pausing, a preserved
  source slot, modifier-release drop, Escape cancellation, and smooth landing.
- **Native smart borders** (`layout { full-width-to-edges }`): single-window
  full-width columns blend continuously to the screen edges as they scroll into
  view, replacing external maximize/unmaximize toggling. See below.
- Configuration validation, lifecycle cleanup, and regression/property coverage.

### Super + three-finger carry

1. Focus a window; hold **Super before beginning a three-finger swipe**.
2. Finger movement progressively lifts/shrinks a live copy toward screen center.
   The actual window stays in its original layout slot as a fading ghost.
3. Horizontal/vertical strokes pan columns/workspaces **underneath the copy** at
   the same time. The desktop does not zoom or open the overview.
4. A brief motion pause or finger-up smoothly completes any partial pickup.
   Keep Super held to lift/reposition fingers and perform additional strokes.
5. Release Super to commit the center insertion preview. The copy moves and
   grows into the chosen slot over **300 ms** by default; the real destination
   tile is hidden during that transition to avoid double drawing.
6. Escape cancels an active carry and restores its original slot immediately.

Ordinary unmodified gestures retain niri's defaults unless explicitly rebound.
The carried app retains focus; ordinary keyboard presses are consumed during
carry mode so they do not type into or move background windows.

### Native smart borders: `full-width-to-edges`

```kdl
layout {
    full-width-to-edges
}
```

A normal column that is full width (for example after `maximize-column`, Mod+F)
and contains exactly one tiled window expands that window to the edges of the
available area as the column scrolls into its aligned position. Gaps, border,
focus ring, shadow, and rounded corners fade out; the bar stays visible.
Scrolling away blends it back. Progress follows the scroll position frame by
frame, including during touchpad swipes, so it is continuous rather than a
toggle on focus change. The blend spans the last quarter of the screen width
before alignment, and at rest the window is always exactly normal or exactly at
the edges, so a partly visible full-width neighbor keeps normal gaps.

- The column keeps its normal gapped layout slot: **neighboring columns never
  move**, and nothing re-lays out when you navigate.
- The window is asked for the edge-to-edge size **once**, when the column becomes
  eligible, and is not resized while navigating. This removes the configure churn
  and resize jank of an external helper toggling maximize-to-edges.
- Mid-scroll, the window is presented scaled by a few percent between the two
  rectangles (slightly softer while moving); when aligned, it is drawn 1:1.
  Pointer input maps through the presented geometry.
- Stacked/tabbed columns, floating, fullscreen, and maximized-to-edges windows are
  unaffected; windows whose max size is smaller than the edges are left alone.
  Leaving eligibility keeps the slot until the client commits its new size, so
  the column never momentarily widens. The option is off by default and can be
  set per output/workspace through normal layout overrides.
- The blend follows horizontal scrolling only, and also applies in the overview.

This supersedes the earlier external smart-border service; disable that service
when using this build.

## Build this branch

Install the normal [niri build dependencies](docs/wiki/Getting-Started.md), then:

```sh
git clone --branch feat/configurable-touchpad-carry-v26.04 https://github.com/sortedcord/niri.git niri-carry
cd niri-carry
cargo build --release --locked
./target/release/niri validate --config resources/gesture-prototype/test-config.kdl
```

These commands build a separate binary. They do **not** install it, replace your
existing compositor, or change your desktop session. Avoid replacing a working
compositor until you have tested this build and have a recovery route.

## Configure

The new syntax is specific to this branch; stock niri v26.04 will reject it.

```kdl
touchpad-gesture fingers=3 modifiers="Super" direction="any" threshold=4.0 sensitivity=1.0 preview-scale=0.85 pickup-distance=120.0 drop-duration-ms=300 {
    carry-window
}
```

- `fingers`: 3–10.
- `modifiers`: exact initial modifier combination, including `Mod`, `Super`,
  `Ctrl`, `Alt`, or `Shift`; defaults to none.
- `direction`: `any`, `horizontal`, `vertical`, `left`, `right`, `up`, or `down`.
- `threshold`: recognition distance; default 16.0.
- `sensitivity`: navigation motion multiplier; default 1.0.
- `preview-scale`: fully lifted size; default 0.85.
- `pickup-distance`: finger travel to complete pickup; default 120.0. Increase it
  for a longer, more gradual lift.
- `drop-duration-ms`: landing duration; default 300. Increase it for a slower
  landing, or set it to 0 for instant drop. Range 0–5000 ms.

Other behaviors are `drag-window`, `scroll-view`, `switch-workspace`, `overview`,
`ignore` (claim and do nothing), `forward-to-client app-id="REGEX"` (hand the swipe to
the window under the pointer, below), and `run { <one normal niri action>; }`.
`modifiers="any"` is a fallback matching any modifiers when no exact binding exists,
e.g. `touchpad-gesture fingers=3 modifiers="any" { ignore; }` disables three-finger
swipes. `carry-window` requires a modifier so it
can commit on release. Defaults remain unchanged if no matching custom binding
exists. Settings apply to new gestures through niri's normal config reload.

`forward-to-client` passes the whole swipe to the application under the pointer as a live
stream: a begin with the finger count, an update with the motion of the fingers on every
touchpad frame, and an end. An app can use it for continuous gestures, such as three-finger
tab scrubbing in Helide. `app-id` is a regular expression for the window's app-id; without
it every window gets the swipe, and windows that do not match are treated as with `ignore`.
The choice is made when the fingers land, so `direction` and `threshold` do not apply:

```kdl
touchpad-gesture fingers=3 modifiers="any" { forward-to-client app-id="^helium$"; }
```

See [the full configuration and behavior documentation](docs/wiki/Configurable-Touchpad-Gestures-Prototype.md)
and [the example bindings](resources/gesture-prototype/gestures.kdl).

## Test in a separate session

A stock outer compositor consumes three-finger gestures, so a nested window is
not a reliable hardware test. Save your work, log out of the graphical session,
and log in on an unused TTY. From the repository directory, run:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY -u NIRI_SOCKET \
  dbus-run-session -- "$PWD/target/release/niri" \
  --config "$PWD/resources/gesture-prototype/test-config.kdl"
```

Do not use sudo. This deliberately minimal config starts no desktop shell or
background services and does not import a new systemd session environment.

- Super+Return opens Kitty; open several terminals for testing.
- Super+Left/Right focuses columns; Super+Up/Down changes workspace.
- Super+Shift+Up/Down moves windows between workspaces for test setup.
- Super+F toggles full-width columns; Super+Space toggles floating.
- **Super+Shift+Escape exits**; cancel an active carry first.

Check gradual pickup, a non-collapsing source slot, simultaneous background
navigation, repeated strokes, modifier-release landing, Escape cancellation,
normal gestures, and cleanup if a window closes or a monitor is removed.

## Validation

The prototype was manually tested on one Linux/niri setup. Before publication,
the full Cargo workspace test suite passed, and the randomized layout operation
test passed with 1,000 cases. It is still experimental and needs validation on
other touchpads, display scales, drivers, and applications.

```sh
cargo test --all --locked
RUN_SLOW_TESTS=1 PROPTEST_CASES=1000 cargo test -p niri --lib layout::tests::random_operations_dont_panic --locked -- --exact
```

Smart-border tests cover edge sizing with an unchanged slot, identical neighbor
positions with/without the option, continuous blending while scrolling, restoring
on return without client resizes, eligibility rules, leaving full width, stacking,
fullscreen, floating, moving between workspaces, and exact pointer mapping
through the scaled presentation. Randomized layout tests include the option.

The tests include recognition/config parsing, modifier release, continuous
pickup, idle completion, source-slot preservation, insertion anchors, landing
geometry/timing, partial pickup release, client resizing, interruption, window
close, and output unplug. Property tests check ghost/hidden-tile cleanup.

## Limitations and credits

- Swipe mappings only: not a general touchscreen, pinch, or rotation framework.
- Carry/direct dragging stays on the starting display.
- The ghost is live, not a frozen screenshot; external app geometry changes can
  still affect its source tile.
- Escape cancellation remains immediate. Global/window-movement animation
  disable is honored for the landing transition.
- A new pickup/native window drag safely interrupts a landing whose layout is
  already committed. Not every hardware/application interaction has been tested.

Upstream niri provides the compositor, layout, navigation, rendering, and
insertion primitives. Parameterized gesture matching and continuous/discrete
dispatch design reference [julianjc84/niri](https://github.com/julianjc84/niri),
branch `feat/configurable-touch-gestures`, reference `464f45a`. That fork's wider
touchscreen/IPC subsystem was not imported. Existing upstream licensing is
retained; see [LICENSE](LICENSE).
