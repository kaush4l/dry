# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (`themes/active` names it); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.

## Colors

| token | light | dark | default light | default dark | use |
|---|---|---|---|---|---|
| background_body | #FBF3E8FF | #1B1512FF | #FBF3E8FF | #1B1512FF | Window behind everything (opaque themes) |
| background_surface | #FFFAF3FF | #2A201AFF | #FFFAF3FF | #2A201AFF | Cards and inputs |
| background_muted | #5A32190A | #FFE9D00D | #5A32190A | #FFE9D00D | Quiet fills |
| overlay_hover | #3A2A200A | #FFE9D014 | #3A2A200A | #FFE9D014 | Hover wash on buttons |
| overlay_pressed | #3A2A2014 | #FFE9D022 | #3A2A2014 | #FFE9D022 | Pressed wash on buttons |
| text_primary | #3A2A20FF | #F6EADBFF | #3A2A20FF | #F6EADBFF | Body text |
| text_secondary | #7A6252FF | #CDB8A3FF | #7A6252FF | #CDB8A3FF | Hints, labels |
| text_disabled | #B9A595FF | #7E6A5AFF | #B9A595FF | #7E6A5AFF | Disabled text |
| text_accent | #B4532AFF | #FFB37AFF | #B4532AFF | #FFB37AFF | Links, active nav text |
| accent | #D9733FFF | #E8875AFF | #D9733FFF | #E8875AFF | Primary buttons, focus ring (terracotta) |
| accent_muted | #D9733F26 | #E8875A33 | #D9733F26 | #E8875A33 | Active nav fill, selection |
| on_accent | #FFFFFFFF | #2A1A10FF | #FFFFFFFF | #2A1A10FF | Text on accent |
| border | #3A2A2014 | #FFE9D01A | #3A2A2014 | #FFE9D01A | Hairlines |
| border_emphasized | #DCCBB9FF | #6B5444FF | #DCCBB9FF | #6B5444FF | Input borders |
| error | #C8323CFF | #FF6B6BFF | #C8323CFF | #FF6B6BFF | Errors |
| shadow | #5A321914 | #0E080559 | #5A321914 | #0E080559 | Pane shadows (soft, so the scene shows through the glass) |
| window_tint | #FBF1E4D9 | #1E1611F2 | #FBF1E4D9 | #1E1611F2 | Tint over the frosted window (warm) |
| glass_fill | #FFFFFF73 | #FFF3E414 | #FFFFFF73 | #FFF3E414 | Glass pane, bottom |
| glass_sheen | #FFFFFFB3 | #FFF3E426 | #FFFFFFB3 | #FFF3E426 | Glass pane, top (the sheen) |
| glass_edge | #FFFFFFD9 | #FFE9D02E | #FFFFFFD9 | #FFE9D02E | Glass pane rim |
| glass_highlight | #FFFFFFFF | #FFF1DE40 | #FFFFFFFF | #FFF1DE40 | Light caught on a pane's top edge |
| backdrop_1 | #F2A65A59 | #E8875A40 | #F2A65A59 | #E8875A40 | Backdrop light, far (ember) |
| backdrop_2 | #F7D48A59 | #F2C46D33 | #F7D48A59 | #F2C46D33 | Backdrop light (honey) |
| backdrop_3 | #E8907A40 | #B5654A33 | #E8907A40 | #B5654A33 | Backdrop light, near (rose clay) |
| status_bar | #F7EBDDFF | #241A14FF | #F7EBDDFF | #241A14FF | Status strip at the bottom of an app-like page |
| on_status_bar | #7A6252FF | #CDB8A3FF | #7A6252FF | #CDB8A3FF | Text on the status strip |
| line_highlight | #3A2A200F | #FFE9D012 | #3A2A200F | #FFE9D012 | The cursor's line in an editor |
| activity_llm | #D9733F1F | #E8875A2E | #D9733F1F | #E8875A2E | Engine tint: calling the LLM (ginger) |
| activity_tools | #8A5CC21A | #B48CE629 | #8A5CC21A | #B48CE629 | Engine tint: running tools (lavender yarn) |
| activity_compress | #3F8FA61A | #6CC3D929 | #3F8FA61A | #6CC3D929 | Engine tint: compressing the history (teal) |
| activity_wait | #C9A2271F | #E6C25A26 | #C9A2271F | #E6C25A26 | Engine tint: waiting for a callback (honey) |
| activity_work | #5E9C4A1A | #8CCB7029 | #5E9C4A1A | #8CCB7029 | Engine tint: a session program working (catnip) |
| activity_need | #E0A3001F | #FFC94D2E | #E0A3001F | #FFC94D2E | Engine tint: needs you (marigold) |
| activity_fail | #C8323C1F | #FF6B6B2E | #C8323C1F | #FF6B6B2E | Engine tint: failed (berry) |

## Sizes

| token | value | default | use |
|---|---|---|---|
| space_1 | 4 | 4 | Smallest gap |
| space_2 | 8 | 8 | Small gap |
| space_3 | 12 | 12 | Medium gap |
| space_4 | 16 | 16 | Large gap |
| radius | 18 | 18 | Control corners (capsule at 30 high) |
| text_sm | 11 | 11 | Small text |
| text_base | 14 | 14 | Body text size |
| text_code | 13 | 13 | Code: editor lines and terminal output |
| text_lg | 28 | 28 | Page titles |
| pane_gap | 14 | 14 | Space between window edge and panes |
| pane_radius | 22 | 22 | Glass pane corners |
| nav_pinned | 0 | 0 | Navigation: 1 pinned sidebar, 0 floating button |

## Font

| token | value | default | use |
|---|---|---|---|
| font | Georgia | Georgia | Every text in the app |
| font_mono | Menlo | Menlo | Code, line numbers and terminal output |
