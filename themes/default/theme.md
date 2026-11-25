# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (app.md's `theme:` names its folder); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.

## Colors

| token | light | dark | default light | default dark | use |
|---|---|---|---|---|---|
| background_body | #F1F4F7FF | #000000FF | #F1F4F7FF | #000000FF | Window behind everything (opaque themes) |
| background_surface | #FFFFFFFF | #0A0A0AFF | #FFFFFFFF | #0A0A0AFF | Cards and inputs |
| background_muted | #0536590C | #1111127F | #0536590C | #1111127F | Quiet fills |
| overlay_hover | #0000000A | #FFFFFF12 | #0000000A | #FFFFFF12 | Hover wash on buttons |
| overlay_pressed | #00000014 | #FFFFFF1F | #00000014 | #FFFFFF1F | Pressed wash on buttons |
| text_primary | #1D1D1FFF | #F5F5F7FF | #1D1D1FFF | #F5F5F7FF | Body text |
| text_secondary | #6E6E73FF | #AEAEB2FF | #6E6E73FF | #AEAEB2FF | Hints, labels |
| text_disabled | #A4B0BCFF | #6F747CFF | #A4B0BCFF | #6F747CFF | Disabled text |
| text_accent | #0066CCFF | #409CFFFF | #0066CCFF | #409CFFFF | Links, active nav text |
| accent | #0A84FFFF | #0A84FFFF | #0A84FFFF | #0A84FFFF | Primary buttons, focus ring |
| accent_muted | #0A84FF26 | #0A84FF33 | #0A84FF26 | #0A84FF33 | Active nav fill, selection |
| on_accent | #FFFFFFFF | #FFFFFFFF | #FFFFFFFF | #FFFFFFFF | Text on accent |
| border | #0000000F | #FFFFFF14 | #0000000F | #FFFFFF14 | Hairlines |
| border_emphasized | #CCD3DBFF | #494D53FF | #CCD3DBFF | #494D53FF | Input borders |
| error | #E3193BFF | #F5394FFF | #E3193BFF | #F5394FFF | Errors |
| shadow | #0A1E3C14 | #000000CC | #0A1E3C14 | #000000CC | Pane shadows |
| window_tint | #F5F7FA33 | #000000FF | #F5F7FA33 | #000000FF | Tint over the frosted window |
| glass_fill | #FFFFFF66 | #FFFFFF0A | #FFFFFF66 | #FFFFFF0A | Glass pane, bottom |
| glass_sheen | #FFFFFFA6 | #FFFFFF1C | #FFFFFFA6 | #FFFFFF1C | Glass pane, top (the sheen) |
| glass_edge | #FFFFFFCC | #FFFFFF14 | #FFFFFFCC | #FFFFFF14 | Glass pane rim |
| glass_highlight | #FFFFFFFF | #FFFFFF38 | #FFFFFFFF | #FFFFFF38 | Light caught on a pane's top edge |
| backdrop_1 | #5AC8FA66 | #0A84FF66 | #5AC8FA66 | #0A84FF66 | Backdrop light, far (blue) |
| backdrop_2 | #FF7EB866 | #BF5AF259 | #FF7EB866 | #BF5AF259 | Backdrop light (pink) |
| backdrop_3 | #6EE7B766 | #30D1584D | #6EE7B766 | #30D1584D | Backdrop light, near (mint) |
| status_bar | #F5F7FAFF | #0A0A0AFF | #F5F7FAFF | #0A0A0AFF | Status strip at the bottom of an app-like page |
| on_status_bar | #6E6E73FF | #AEAEB2FF | #6E6E73FF | #AEAEB2FF | Text on the status strip |
| line_highlight | #0000000F | #FFFFFF14 | #0000000F | #FFFFFF14 | The cursor's line in an editor |
| activity_llm | #0A84FF1F | #0A84FF2E | #0A84FF1F | #0A84FF2E | Engine tint: calling the LLM (blue) |
| activity_tools | #BF5AF21F | #BF5AF22E | #BF5AF21F | #BF5AF22E | Engine tint: running tools (purple) |
| activity_compress | #5E5CE61F | #5E5CE633 | #5E5CE61F | #5E5CE633 | Engine tint: compressing the history (indigo) |
| activity_wait | #FF9F0A1F | #FF9F0A29 | #FF9F0A1F | #FF9F0A29 | Engine tint: waiting for a terminal callback (amber) |
| activity_work | #30D1581F | #30D15829 | #30D1581F | #30D15829 | Engine tint: a session program working (green) |
| activity_need | #FFCC0029 | #FFD60A2E | #FFCC0029 | #FFD60A2E | Engine tint: needs you (yellow) |
| activity_fail | #FF3B301F | #FF453A2E | #FF3B301F | #FF453A2E | Engine tint: failed (red) |

## Sizes

| token | value | default | use |
|---|---|---|---|
| space_1 | 4 | 4 | Smallest gap |
| space_2 | 8 | 8 | Small gap |
| space_3 | 12 | 12 | Medium gap |
| space_4 | 16 | 16 | Large gap |
| radius | 15 | 15 | Control corners (capsule at 30 high) |
| text_sm | 11 | 11 | Small text |
| text_base | 13 | 13 | Body text size |
| text_code | 13 | 13 | Code: editor lines and terminal output |
| text_lg | 26 | 26 | Page titles |
| pane_gap | 12 | 12 | Space between window edge and panes |
| pane_radius | 16 | 16 | Glass pane corners |
| nav_pinned | 0 | 0 | Navigation: 1 pinned sidebar, 0 floating button |

## Font

| token | value | default | use |
|---|---|---|---|
| font | .SystemUIFont | .SystemUIFont | Every text in the app |
| font_mono | Menlo | Menlo | Code, line numbers and terminal output |
