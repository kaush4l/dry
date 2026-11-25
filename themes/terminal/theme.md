# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (app.md's `theme:` names its folder); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.

## Colors

| token | light | dark | default light | default dark | use |
|---|---|---|---|---|---|
| background_body | #FAFAF5FF | #000000FF | #FAFAF5FF | #000000FF | Window behind everything (opaque themes) |
| background_surface | #FFFFFFFF | #000000E6 | #FFFFFFFF | #000000E6 | Cards and inputs |
| background_muted | #0000000A | #33FF660F | #0000000A | #33FF660F | Quiet fills |
| overlay_hover | #00000010 | #33FF661A | #00000010 | #33FF661A | Hover wash on buttons |
| overlay_pressed | #00000020 | #33FF6633 | #00000020 | #33FF6633 | Pressed wash on buttons |
| text_primary | #000000FF | #33FF66FF | #000000FF | #33FF66FF | Body text |
| text_secondary | #3A3A3AFF | #22B348FF | #3A3A3AFF | #22B348FF | Hints, labels |
| text_disabled | #8C8C8CFF | #1A6630FF | #8C8C8CFF | #1A6630FF | Disabled text |
| text_accent | #0A6B1FFF | #FFB000FF | #0A6B1FFF | #FFB000FF | Links, active nav text |
| accent | #0A6B1FFF | #33FF66FF | #0A6B1FFF | #33FF66FF | Primary buttons, focus ring |
| accent_muted | #0A6B1F26 | #33FF6633 | #0A6B1F26 | #33FF6633 | Active nav fill, selection |
| on_accent | #FFFFFFFF | #000000FF | #FFFFFFFF | #000000FF | Text on accent |
| border | #00000033 | #33FF6640 | #00000033 | #33FF6640 | Hairlines |
| border_emphasized | #000000B3 | #33FF66B3 | #000000B3 | #33FF66B3 | Input borders |
| error | #C4001AFF | #FF3B30FF | #C4001AFF | #FF3B30FF | Errors |
| shadow | #00000000 | #33FF6614 | #00000000 | #33FF6614 | Pane shadows (a faint phosphor halo) |
| window_tint | #FAFAF5FF | #000000FF | #FAFAF5FF | #000000FF | Tint over the frosted window |
| glass_fill | #FAFAF5D9 | #000000C7 | #FAFAF5D9 | #000000C7 | Glass pane, bottom |
| glass_sheen | #FAFAF5D9 | #000000C7 | #FAFAF5D9 | #000000C7 | Glass pane, top (the sheen) |
| glass_edge | #00000059 | #33FF6666 | #00000059 | #33FF6666 | Glass pane rim |
| glass_highlight | #00000000 | #00000000 | #00000000 | #00000000 | Light caught on a pane's top edge |
| backdrop_1 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light, far (none: flat) |
| backdrop_2 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light (none) |
| backdrop_3 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light, near (none) |
| status_bar | #FFFFFFFF | #000000E6 | #FFFFFFFF | #000000E6 | Status strip at the bottom of an app-like page |
| on_status_bar | #3A3A3AFF | #22B348FF | #3A3A3AFF | #22B348FF | Text on the status strip |
| line_highlight | #00000014 | #33FF6626 | #00000014 | #33FF6626 | The cursor's line in an editor |
| activity_llm | #0A6B1F1A | #33FF6626 | #0A6B1F1A | #33FF6626 | Engine tint: calling the LLM (phosphor) |
| activity_tools | #9A6A001A | #FFB00026 | #9A6A001A | #FFB00026 | Engine tint: running tools (amber) |
| activity_compress | #00708A1A | #33D6FF22 | #00708A1A | #33D6FF22 | Engine tint: compressing the history (cyan) |
| activity_wait | #4A4A4A1A | #9CFF9C1A | #4A4A4A1A | #9CFF9C1A | Engine tint: waiting for a callback (dim phosphor) |
| activity_work | #1F7A3A1A | #66FF991F | #1F7A3A1A | #66FF991F | Engine tint: a session program working (green) |
| activity_need | #B358001F | #FF8C1A2E | #B358001F | #FF8C1A2E | Engine tint: needs you (orange) |
| activity_fail | #C4001A1F | #FF3B302E | #C4001A1F | #FF3B302E | Engine tint: failed (red) |

## Sizes

| token | value | default | use |
|---|---|---|---|
| space_1 | 4 | 4 | Smallest gap |
| space_2 | 8 | 8 | Small gap |
| space_3 | 12 | 12 | Medium gap |
| space_4 | 16 | 16 | Large gap |
| radius | 0 | 0 | Control corners (capsule at 30 high) |
| text_sm | 11 | 11 | Small text |
| text_base | 13 | 13 | Body text size |
| text_code | 13 | 13 | Code: editor lines and terminal output |
| text_lg | 20 | 20 | Page titles |
| pane_gap | 8 | 8 | Space between window edge and panes |
| pane_radius | 2 | 2 | Glass pane corners |
| nav_pinned | 1 | 1 | Navigation: 1 pinned sidebar, 0 floating button |

## Font

| token | value | default | use |
|---|---|---|---|
| font | Menlo | Menlo | Every text in the app |
| font_mono | Menlo | Menlo | Code, line numbers and terminal output |
