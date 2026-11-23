# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (`themes/active` names it); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.

## Colors

| token | light | dark | default light | default dark | use |
|---|---|---|---|---|---|
| background_body | #E6F7F8FF | #020611FF | #E6F7F8FF | #020611FF | Window behind everything (opaque themes) |
| background_surface | #F4FCFDFF | #061428FF | #F4FCFDFF | #061428FF | Cards and inputs |
| background_muted | #04202A0A | #4DF3FF0D | #04202A0A | #4DF3FF0D | Quiet fills |
| overlay_hover | #04202A0D | #4DF3FF14 | #04202A0D | #4DF3FF14 | Hover wash on buttons |
| overlay_pressed | #04202A1A | #4DF3FF24 | #04202A1A | #4DF3FF24 | Pressed wash on buttons |
| text_primary | #04202AFF | #D8FBFFFF | #04202AFF | #D8FBFFFF | Body text |
| text_secondary | #2E6170FF | #7FB8C8FF | #2E6170FF | #7FB8C8FF | Hints, labels |
| text_disabled | #8DB1BAFF | #3E6372FF | #8DB1BAFF | #3E6372FF | Disabled text |
| text_accent | #B0228AFF | #FF6FE0FF | #B0228AFF | #FF6FE0FF | Links, active nav text (magenta glow) |
| accent | #008C9EFF | #18D6E6FF | #008C9EFF | #18D6E6FF | Primary buttons, focus ring (bioluminescent teal) |
| accent_muted | #008C9E2E | #18D6E633 | #008C9E2E | #18D6E633 | Active nav fill, selection |
| on_accent | #FFFFFFFF | #001218FF | #FFFFFFFF | #001218FF | Text on accent |
| border | #04202A17 | #4DF3FF1F | #04202A17 | #4DF3FF1F | Hairlines |
| border_emphasized | #A6CDD4FF | #1F5566FF | #A6CDD4FF | #1F5566FF | Input borders |
| error | #C8205AFF | #FF4F7AFF | #C8205AFF | #FF4F7AFF | Errors |
| shadow | #0A3A4A14 | #00040C40 | #0A3A4A14 | #00040C40 | Pane shadows (soft, so the scene shows through the glass) |
| window_tint | #DDF4F5D9 | #030A1AF5 | #DDF4F5D9 | #030A1AF5 | Tint over the frosted window (ink navy) |
| glass_fill | #FFFFFF59 | #0B2A3A40 | #FFFFFF59 | #0B2A3A40 | Glass pane, bottom |
| glass_sheen | #FFFFFF99 | #1FE3FF1F | #FFFFFF99 | #1FE3FF1F | Glass pane, top (the sheen) |
| glass_edge | #FFFFFFCC | #3FF2FF40 | #FFFFFFCC | #3FF2FF40 | Glass pane rim |
| glass_highlight | #FFFFFFFF | #7FF7FF66 | #FFFFFFFF | #7FF7FF66 | Light caught on a pane's top edge |
| backdrop_1 | #5AD8E866 | #0A6E8A66 | #5AD8E866 | #0A6E8A66 | Backdrop light, far (deep teal) |
| backdrop_2 | #F28AD24D | #8A1E7A4D | #F28AD24D | #8A1E7A4D | Backdrop light (magenta) |
| backdrop_3 | #7AB8FF4D | #0F3D8A66 | #7AB8FF4D | #0F3D8A66 | Backdrop light, near (trench blue) |
| status_bar | #DDF0F3FF | #04101FFF | #DDF0F3FF | #04101FFF | Status strip at the bottom of an app-like page |
| on_status_bar | #2E6170FF | #7FB8C8FF | #2E6170FF | #7FB8C8FF | Text on the status strip |
| line_highlight | #04202A0F | #4DF3FF14 | #04202A0F | #4DF3FF14 | The cursor's line in an editor |
| activity_llm | #008C9E1F | #18D6E62E | #008C9E1F | #18D6E62E | Engine tint: calling the LLM (bioluminescent cyan) |
| activity_tools | #5B4BD11A | #8C7CFF2E | #5B4BD11A | #8C7CFF2E | Engine tint: running tools (violet jelly) |
| activity_compress | #1E6FD91A | #4DA3FF29 | #1E6FD91A | #4DA3FF29 | Engine tint: compressing the history (deep blue) |
| activity_wait | #0E7C6B1A | #3DFFD61F | #0E7C6B1A | #3DFFD61F | Engine tint: waiting for a callback (kelp) |
| activity_work | #00A3881A | #2EE6B829 | #00A3881A | #2EE6B829 | Engine tint: a session program working (sea glass) |
| activity_need | #D99A001F | #FFC24D29 | #D99A001F | #FFC24D29 | Engine tint: needs you (anglerfish lure) |
| activity_fail | #C8205A1F | #FF4F7A2E | #C8205A1F | #FF4F7A2E | Engine tint: failed (coral red) |

## Sizes

| token | value | default | use |
|---|---|---|---|
| space_1 | 4 | 4 | Smallest gap |
| space_2 | 8 | 8 | Small gap |
| space_3 | 12 | 12 | Medium gap |
| space_4 | 16 | 16 | Large gap |
| radius | 10 | 10 | Control corners (capsule at 30 high) |
| text_sm | 11 | 11 | Small text |
| text_base | 13 | 13 | Body text size |
| text_code | 13 | 13 | Code: editor lines and terminal output |
| text_lg | 30 | 30 | Page titles |
| pane_gap | 14 | 14 | Space between window edge and panes |
| pane_radius | 20 | 20 | Glass pane corners |
| nav_pinned | 0 | 0 | Navigation: 1 pinned sidebar, 0 floating button |

## Font

| token | value | default | use |
|---|---|---|---|
| font | Avenir Next | Avenir Next | Every text in the app |
| font_mono | Menlo | Menlo | Code, line numbers and terminal output |
