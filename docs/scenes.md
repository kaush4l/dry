# Scenes: animated backgrounds, apart from the theme

A scene is something that moves behind the glass: a cat walking the floor,
a phosphor terminal at rest, the deep sea. Which scene is an app setting,
not part of the theme: any theme shows any scene. The scene owns its
colours (a light and a dark one per key); a key can name a role, a
standard theme token such as `accent`, and then wears the theme's colour.
Appearance (light/dark, motion) is an input it is drawn with
(owner, 2026-11-23).

This replaces the earlier eggs draft. "Eggs" are now called scenes.

## What ships

| Scene | What moves | Colours | fps |
|---|---|---|---|
| `cat` (the default, `default/app.md`) | dry's first cat (`app/eggs/pet`), same body and ways: it chases, jumps for and pounces on the pointer, purrs hearts, naps; a `!` when the engines need you | roles: `accent`, `on_accent`, `text_accent`, `error`, so it wears the theme's highlight (terracotta on `cat`, blue on `default`) | 60 |
| `terminal` | a block cursor wanders and types short glowing traces (word-shaped bars, not letters) that fade like phosphor; a slow scanline sweeps down; some lines are amber prompts. Nothing falls | its own: phosphor green and amber | 24 |
| `abyss` | jellyfish rise in pulses (each contraction of the bell is a surge upward), trailing swaying tentacles; plankton motes drift up and twinkle; faint light shafts lean in from above | its own cyan and magenta; the bell's inside is the theme's `accent_muted` | 30 |

The themes (`default`, `cat`, `terminal`, `abyss`) no longer name a scene;
the `cat`, `terminal` and `abyss` themes are the looks the scenes were made
with. `cargo run -p dry-tools -- scenes` lists the scenes, checks each one
and marks the one in force; `dry scene use abyss` (or `none`) and
`dry motion 0` set the app's settings.

## The cat: dry's first cat, ported exactly

The cat is the one from dry (`app/eggs/pet/egg.js`), with the same body,
the same states, the same constants and the same physics. Everything is
in CSS pixels, so `scenes/cat.md` says `cell 1`. Its body is made of parts:
legs, tail, body, ears and head. Each part is drawn twice, first fattened
in the outline colour and then filled, so the cat gets one sticker
outline. The part list is in `scenes/cat.md` under "Body".

| Trick | What it does (egg.js's numbers) |
|---|---|
| Look | Sitting, it turns its head to the pointer, up to 0.7 rad up; its eyes look up when the head does |
| Chase | It runs under the pointer (kept 24 px from the sides). Its speed comes from the distance, `min(480, 30 + 2.6·d)` px/s, reached at up to 1500 px/s². It starts once the pointer is 16 px away and sits within 2.5 px. After a quiet spell (more than 1.5 s) it waits 260 ms to notice first. A run longer than 120 px calms it |
| Jump | When the pointer is 50-340 px over the floor and in reach, it crouches for 230 ms and jumps in an arc with gravity 2600 px/s². The arc's top is the pointer's height less 44 px, plus 4 px (at least 14 px). It flies at most 320 px/s sideways. In the air its paws reach for the pointer, its spine arches most at the top and its hind legs tuck. It lands with a squash, then the tail and head follow through with a damped bounce, and it waits 350 ms before it can jump again |
| Misses | Too high, it jumps anyway and misses. Its reach is 70 px, plus 40 px per miss, up to 3 misses. It gets three tries at a spot, and a pointer that moves 24 px counts as a new spot |
| Pounce | When the pointer is held still in reach for 2.6 s, it stalks (low, wiggling, its tail flicking), then pounces after a quicker 120 ms crouch. It catches the pointer at the top of the arc, with 8 hearts, and then purrs |
| Purr | Click on it (within 40 px, from its feet to 60 px over its head) while it sits, stalks or sleeps, and it purrs for 1.5 s. It wiggles, and a heart pops in every 240 ms for the first second. Each heart drifts, wobbles as it rises and fades over 1.4 s |
| Rest | Sitting still, its loop parks. A timer every 2.5-6.5 s wakes it for a blink (150 ms) or a tail swish (2.4 s). After 2.5 s with no input its ears droop |
| Nap | At the first timer after 20 s with no input, it lies down with its eyes shut and a `z z` over it. Nothing is drawn after that. Any pointer move wakes it, after the 260 ms beat |
| Edges | When the pointer leaves the window within 40 px of a side, the cat runs to that edge |
| Still | `motion` 0 and reduced motion: it sits still and only turns its head (and faces) to follow the pointer. It never runs, jumps or purrs |

**From the engines** (`scene::mood`, Home's tiles' rule), only things that
do not change the first cat's behaviour:
- **Needs you:** a `!` (the theme's `accent`) over its head.
- **Done** (working, then idle): it purrs, as it does for a click.
- **Working:** nothing. The old pacing would have broken the first cat's
  sit-and-nap rule, so it is gone.

**Time.** In egg.js, `update(dt)` runs once per animation frame. Here
the same `update` runs on a fixed 120 Hz tick. `Live` holds egg.js's
variables as they were at the last input change. The frame at `t` runs
the ticks (and the rest timers) from there to `t`, so the pose is a pure
function of `t` and the inputs, the same at 24 and 60 fps. As in egg.js,
the loop parks: `Frame::next` is `Some(t)` while it moves, the timer's
time while it sits, and `None` asleep. Chance is a seeded xorshift, so
the timers are repeatable.

**Drawing.** The canvas paths of egg.js are filled into a pixel grid in
egg.js's order: ellipses, round strokes along lines and quadratic
curves, the ear triangles, the eye arcs and the hearts. Each run of one
colour along a row is one rectangle, so the renderers still only paint
rectangles. The grid is the file's `pixel` (1 px) at 60 fps, and twice
as coarse below. A pixel is filled when at least a third of it is
covered, so the 1.3 px eye lines and a blink stay visible. A frame is
about 280 rectangles.

It stands on the footer row's top edge, less one pixel, as in egg.js.
The floor is an input the shell sets.

## Which scene: an app setting

The scene and the motion are app settings, layered like the others,
nearest first (`dry_core::scene::settings`):

1. a `## Scene` table in an override set: this machine's `state/local.md`
   (wins), then the route's `overrides.md`;
2. the store's `app.md`;
3. the shipped default's `app.md` (`default/app.md`: `scene: cat`,
   `motion: 40`), which reads under a store's `app.md` setting by setting
   (`dry_core::store::merge`), and which a save does not copy into it
   (`split`);
4. for the motion only, an older theme file's `motion` size row.

`app.md`:

```
mode: dark
scene: cat
motion: 40

engines:
- translator
```

`state/local.md` (or a route's `overrides.md`):

```
## Scene

| setting | value |
|---|---|
| scene | none |
| motion | 0 |
```

`none` is no scene. `motion` is seconds per loop of the slowest backdrop
layer; 0 keeps the backdrop and the scene fully still (the code and live
pages ship `| motion | 0 |`). Still does not mean blank: the cat still
poses by mood (asleep, sitting, alert). The system's reduced-motion
setting stills them too. The UI tokens page (`/settings/ui`) shows both
settings under the tokens (the window edits them; they are saved with the
state), and says when the named scene is missing or does not read.

The landing page names its scene itself (`landing('scene', 'abyss',
'abyss')`): the abyss scene on the abyss theme.

## Its colours

Each palette key has its own light and dark colour, and may name a role:

```
| key | role | light | dark | use |
|---|---|---|---|---|
| f | accent | #D9733F | #E8875A | Fur |
| j | - | #00A9C0 | #3FF8FF | Cyan glow |
```

The colour drawn (`dry_core::scene::colours`, pure: palette, the theme's
tokens and the mode → key to colour): the theme's value of the role for
the mode when the theme has that token, else the scene's own colour for
the mode. `-` is no role: always its own. A role is one of the standard
colour tokens every theme has (`accent`, `on_accent`, `text_accent`,
`error`, `text_primary`, `accent_muted`, …). The renderers resolve again
whenever the mode or the theme changes; on the server's pages the
colours are in the page's tokens as `--scene-<key>` per mode.

**Older files.** A palette row in the old `| key | token | use |` form
still reads, as a role with no colour of its own (clear where the theme
lacks it); the old `scene_1` ... `scene_6` tokens are gone, so a row
naming one is an error that says to give the key its own colours. A theme
or override file that still has `scene_1` ... `scene_6` rows, a
`## Scene` table or a `motion` size row reads without error: the inks and
the theme's scene row are dropped, the motion row is honoured (step 4
above), and none of them is written back when the theme is saved.

**Glass rule:** a theme keeps its pane shadow light, so a scene shows. In GPUI
a drop shadow is painted under the translucent glass too, so a dark
shadow (`#…CC`) hides the scene behind the pane. `cat` and `abyss` use
about 25% opacity, and `terminal` keeps it almost clear.

## A scene file

`scenes/<name>.md` is data:

- **`## Scene`** rows:
  - `kind`: `cat`, `terminal` or `abyss`, the behaviour that moves it;
  - `place`: `window` (the whole background, behind the panes), `front`
    (the whole window over the panes, taking no pointer events: the cat)
    or `footer` (the footer row only, behind its text);
  - `fps`;
  - `cell`: logical pixels per cell, the unit of every position, speed
    and jump;
  - `pixel`: logical pixels per sprite pixel (default: the cell). The
    cat is in 1 px cells and is filled into a 1 px grid (0.5 is finer).
- **`## Palette`:** `| key | role | light | dark | use |`, one character
  to its colours (see "Its colours").
- **`## Sprites`:** a `### <block>` heading, then one fenced block per
  frame, one string per row, with `.` for clear.

It is checked as a whole when it is read, and an error says what is wrong:
- a known kind;
- fps, cell and pixel above 0;
- every palette role is a real colour token (or `-`), and every colour
  is `#RRGGBB` or `#RRGGBBAA`;
- every key the sprites and the code use is in the palette;
- every block its kind needs is there, with enough frames;
- the frames of a block are all one size.

The behaviours are the only code: `crates/core/src/scene/{cat,terminal,abyss}.rs`.

## fps buys quality, never speed

- **All motion is a pure function of `t`,** seconds on the scene's clock.
  Positions and phases come from `t`, never from a frame counter.
- **A footer scene runs at 24 fps.** A window scene declares its own fps.
- **`detail(fps)` is the tier a frame rate buys:** 1 below 30 fps, 2 from
  30, 3 from 60. A higher tier adds detail and never changes where
  anything is:

| | 24 fps (tier 1) | 30 fps (tier 2) | 60 fps (tier 3) |
|---|---|---|---|
| cat | the body filled on a 2 px grid | the same | a 1 px grid (the file's `pixel`) |
| terminal | flat traces, one-row scanline | + glow, three-row scanline tail | + cursor bloom, six-row tail |
| abyss | 36 motes, 2 bell frames, 3 tentacle segments, coarse shafts | + bell glow, 70 motes, 5 segments, finer shafts | + 4 bell frames, 120 motes, 8 segments, fine shafts |

The walk frame comes from how far the cat has walked, and the bell frame
from where it is in its pulse, so these too are the same at any fps.

The test `the_same_t_gives_the_same_pose_at_any_fps_and_more_fps_more_detail`
(`crates/core/src/scene/mod.rs`) shows this. It steps every shipped scene
at 24 and at 60 fps through the same input changes at the same times
(mood changes, and a pointer that sweeps, holds still over the cat,
clicks it, leaves the window and comes back), and checks three things:
- the memory is equal, and the positions (`Frame::anchors`: the cat and
  each heart) are equal at every 1/12 s, a frame at both rates;
- the cat followed the sweep, and "done" purred;
- the 60 fps frame has more draws.

## Where each part lives

| Part | Where | What it does |
|---|---|---|
| Format, mood, inputs, frame, tiers | `crates/core/src/scene/mod.rs` | Pure: no I/O, no GPUI; tested with a fake clock |
| Behaviours | `crates/core/src/scene/{cat,terminal,abyss}.rs` | Pure |
| Scene files | `scenes/*.md` | Data |
| The settings (`settings`), the palette (`Ink`, `colours`) | `crates/core/src/scene/mod.rs` | Pure |
| `app.md`'s `scene:` / `motion:` | `crates/core/src/files.rs` (`app_md`, `parse_settings`), layered in `crates/core/src/store.rs` | Pure |
| A `## Scene` override row | `crates/core/src/theme.rs` (`Overrides`) | Pure |
| Read `scenes/` | `dry_tools::{list_scenes, read_scene}` | Disk; the app re-reads it on the 1 s file watch, with the themes |
| Draw and loop | `crates/gpui/src/scene.rs` (`SceneView`) | Only draws |
| Where it is layered | `crates/gpui/src/frame.rs` (`Parts::scene`) | Window: after the backdrop, before the pane. Footer: in the footer row, under its text |
| Draw and loop, browser build | `crates/web/src/scene.rs` | The same calls on a `<canvas>` behind the glass |
| Draw and loop, server pages | `crates/scene` → `crates/server/assets/scene.wasm`, `crates/server/assets/scene.js` | The core's scene code compiled to a small module (`scripts/scene-wasm.sh`); the page feeds it the mood (`<html data-mood>`), the window, the floor and the pointer, and paints. docs/web.md, "The scene" |

**The floor on the web.** Both web builds put it where the window puts it:
the footer row's top edge (`#footer`'s top, else `layout::INSET +
FOOTER_HEIGHT` above the bottom), floored to a whole cell, so the cat
stands on the footer instead of under it. The cat's cell is 1 px, so its
floor is the footer's top, and it stands one pixel above it, as in egg.js
(not measured again on a page since the change).

The core keeps one piece of memory, `Live`. `step(live, inputs, t)` runs
only when the inputs change. `frame(scene, live, t, fps)` returns the
cells to draw, the anchors, and `next`: when the picture changes next,
with `None` meaning not until the inputs change.

**The GPUI loop.** `SceneView` is its own entity with its own clock. On
each app render it gets `sync(scene, mood, still, area, floor)`, and only
a change does anything.

**The pointer in GPUI.** When the scene paints, it registers
window-level mouse listeners (`Window::on_mouse_event`) for move, press,
release and leaving the window. They act in the capture phase, which
runs for every listener back to front before any element handles the
event, and never stop it, so they see the pointer over panes, buttons
and the bare window alike while the UI above gets every click as
before. The scene's canvas adds no hitbox, so it blocks nothing. Each
change becomes a `Pointer` in cells, steps the scene at that moment, and
redraws at once if the picture changed (the eyes turn the frame the
pointer moves); a move far from a sleeping cat changes nothing, so
nothing is drawn and the loop stays parked.

Its loop is a timer task, not
`with_animation(...repeat)`. Each round it:
1. waits until the frame's `next`, and at least one frame at the scene's
   fps;
2. asks for a redraw;
3. ends (parks) when `next` is `None`, when there is no scene, or while
   the window is not visible.

GPUI does expose visibility (`observe_window_visibility`: covered,
minimised, or display asleep). Each cell is one quad painted on a canvas,
and a glow is a blurred shadow in the same colour.

**Renderer-neutral.** A frame is rectangles in cell units with a palette
key, an opacity and an optional glow. A terminal renderer could draw the
same scene as half blocks, and skip the glow.

## Later

- **Location-based scenes** (weather or a map): the weather where you
  are, or a map position. `Inputs` is where they join (for example
  `weather: Option<Weather>` or a position). A scene that does not use
  an input ignores it, so adding one changes nothing for the others.
- **Touch:** the first cat took taps and drags (tap to send it, tap above
  it to make it jump); `Pointer` has no touch yet, so it is not ported.
- **One cat across windows,** as the first cat walked between them (the
  egg channel's token, `egg.pass`, coming in from the facing side at the
  same height). It needs a shared channel between every window that draws
  the scene, and none exists here yet. One window's cat stops at the edge,
  as the first cat did when alone.
- **Break nudges**, from the old journey's rules.
- **A terminal renderer** of the same cells.
- **Cost:** a scene redraw re-renders the whole window, which is already
  true of the moving backdrop. Caching the main view would make a scene
  frame cost only the scene.
- **Needs you:** the cat alone for now; a sound or a notification could
  come later.
