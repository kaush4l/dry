# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (app.md's `theme:` names its folder); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.

## Colors

| token | light | dark | default light | default dark | use |
|---|---|---|---|---|---|
| background_body | #FAFAFAFF | #0A0A0AFF | #FAFAFAFF | #0A0A0AFF | Window behind everything (opaque themes) |
| background_surface | #FFFFFFFF | #171717FF | #FFFFFFFF | #171717FF | Cards and inputs |
| background_muted | #1717170D | #FFFFFF0D | #1717170D | #FFFFFF0D | Quiet fills |
| overlay_hover | #1717170D | #FFFFFF0D | #1717170D | #FFFFFF0D | Hover wash on buttons |
| overlay_pressed | #1717171A | #FFFFFF1A | #1717171A | #FFFFFF1A | Pressed wash on buttons |
| text_primary | #0A0A0AFF | #FFFFFFFF | #0A0A0AFF | #FFFFFFFF | Body text |
| text_secondary | #525252FF | #A3A3A3FF | #525252FF | #A3A3A3FF | Hints, labels |
| text_disabled | #A3A3A3FF | #737373FF | #A3A3A3FF | #737373FF | Disabled text |
| text_accent | #7645E8FF | #9C7DFFFF | #7645E8FF | #9C7DFFFF | Links, active nav text |
| accent | #7645E8FF | #8A5FFFFF | #7645E8FF | #8A5FFFFF | Primary buttons, focus ring |
| accent_muted | #8A5FFF26 | #9C7DFF33 | #8A5FFF26 | #9C7DFF33 | Active nav fill, selection |
| on_accent | #FFFFFFFF | #FFFFFFFF | #FFFFFFFF | #FFFFFFFF | Text on accent |
| border | #1717171A | #FFFFFF1A | #1717171A | #FFFFFF1A | Hairlines |
| border_emphasized | #D4D4D4FF | #404040FF | #D4D4D4FF | #404040FF | Input borders |
| error | #EF4444FF | #F87171FF | #EF4444FF | #F87171FF | Errors |
| shadow | #00000000 | #00000000 | #00000000 | #00000000 | Pane shadows (none: flat) |
| window_tint | #FAFAFAFF | #0A0A0AFF | #FAFAFAFF | #0A0A0AFF | Tint over the frosted window |
| glass_fill | #FFFFFFFF | #171717FF | #FFFFFFFF | #171717FF | Pane fill (flat: no glass) |
| glass_sheen | #FFFFFFFF | #171717FF | #FFFFFFFF | #171717FF | Pane fill, top (the same: no sheen) |
| glass_edge | #1717171A | #FFFFFF1A | #1717171A | #FFFFFF1A | Pane rim (a hairline) |
| glass_highlight | #00000000 | #00000000 | #00000000 | #00000000 | Light on a pane's top edge (none) |
| backdrop_1 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light, far (none: flat) |
| backdrop_2 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light (none) |
| backdrop_3 | #00000000 | #00000000 | #00000000 | #00000000 | Backdrop light, near (none) |
| status_bar | #FAFAFAFF | #0A0A0AFF | #FAFAFAFF | #0A0A0AFF | Status strip at the bottom of an app-like page |
| on_status_bar | #525252FF | #A3A3A3FF | #525252FF | #A3A3A3FF | Text on the status strip |
| line_highlight | #1717170D | #FFFFFF0D | #1717170D | #FFFFFF0D | The cursor's line in an editor |
| activity_llm | #8A5FFF1F | #9C7DFF2E | #8A5FFF1F | #9C7DFF2E | Engine tint: calling the LLM (purple) |
| activity_tools | #1488FC1F | #2BA6FF2E | #1488FC1F | #2BA6FF2E | Engine tint: running tools (blue) |
| activity_compress | #0891B21F | #9AEDFE26 | #0891B21F | #9AEDFE26 | Engine tint: compressing the history (cyan) |
| activity_wait | #D977061F | #F3F99D24 | #D977061F | #F3F99D24 | Engine tint: waiting for a terminal callback (amber) |
| activity_work | #22C55E1F | #4ADE8029 | #22C55E1F | #4ADE8029 | Engine tint: a session program working (green) |
| activity_need | #EAB30829 | #FACC152E | #EAB30829 | #FACC152E | Engine tint: needs you (yellow) |
| activity_fail | #EF44441F | #F871712E | #EF44441F | #F871712E | Engine tint: failed (red) |

## Sizes

| token | value | default | use |
|---|---|---|---|
| space_1 | 4 | 4 | Smallest gap |
| space_2 | 8 | 8 | Small gap |
| space_3 | 12 | 12 | Medium gap |
| space_4 | 16 | 16 | Large gap |
| radius | 8 | 8 | Control corners (capsule at 30 high) |
| text_sm | 11 | 11 | Small text |
| text_base | 13 | 13 | Body text size |
| text_code | 12 | 12 | Code: editor lines and terminal output |
| text_lg | 20 | 20 | Page titles |
| pane_gap | 8 | 8 | Space between window edge and panes |
| pane_radius | 8 | 8 | Glass pane corners |
| nav_pinned | 0 | 0 | Navigation: 1 pinned sidebar, 0 floating button |

## Font

| token | value | default | use |
|---|---|---|---|
| font | .SystemUIFont | .SystemUIFont | Every text in the app |
| font_mono | Menlo | Menlo | Code, line numbers and terminal output |
