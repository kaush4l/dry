# Overrides

The live view is the code view where a person follows an engine's work: the file it is writing, the command it runs, what came back. It is drawn as a flat workbench in the look of `themes/bolt.md` (a depth scale of near-blacks, hairline borders, one purple accent; learned from bolt.new and bolt.diy, docs/loop/bolt-study.md), with the Code page's dense, square sizes, in light and dark, so it follows the app's mode. The colours are written out here so the workbench looks the same whatever theme the rest of the app wears.

## Colors

| token | light | dark | from |
|---|---|---|---|
| background_body | #FFFFFFFF | #0A0A0AFF | the editor: the darkest plane (depth 1) |
| background_surface | #FAFAFAFF | #171717FF | the explorer, the panel and the chat (depth 2) |
| background_muted | #F5F5F5FF | #262626FF | inputs and code blocks (depth 3) |
| overlay_hover | #1717170D | #FFFFFF0D | a row under the pointer |
| overlay_pressed | #1717171A | #FFFFFF1A | the picked row |
| text_primary | #0A0A0AFF | #FFFFFFFF | code and names |
| text_secondary | #525252FF | #A3A3A3FF | labels |
| text_disabled | #A3A3A3FF | #737373FF | line numbers, dimmed lines |
| text_accent | #7645E8FF | #9C7DFFFF | links, the tab in front |
| accent | #7645E8FF | #8A5FFFFF | the one accent: focus, the primary button |
| accent_muted | #8A5FFF26 | #9C7DFF33 | changed lines, selection |
| on_accent | #FFFFFFFF | #FFFFFFFF | text on the accent |
| border | #1717171A | #FFFFFF1A | hairlines between the parts |
| border_emphasized | #D4D4D4FF | #404040FF | input borders |
| error | #EF4444FF | #F87171FF | a failed run |
| window_tint | #FAFAFAFF | #171717FF | flat: no glass |
| glass_fill | #FAFAFAFF | #171717FF | flat: no glass |
| glass_sheen | #FAFAFAFF | #171717FF | flat: no glass |
| glass_edge | #1717171A | #FFFFFF1A | a hairline |
| status_bar | #FAFAFAFF | #171717FF | the status strip |
| on_status_bar | #525252FF | #A3A3A3FF | text on the status strip |
| line_highlight | #1717170D | #FFFFFF0D | the focus lines |
| activity_llm | #8A5FFF1F | #9C7DFF2E | the engine tint, as in themes/bolt.md |
| activity_tools | #1488FC1F | #2BA6FF2E | the engine tint, as in themes/bolt.md |
| activity_compress | #0891B21F | #9AEDFE26 | the engine tint, as in themes/bolt.md |
| activity_wait | #D977061F | #F3F99D24 | the engine tint, as in themes/bolt.md |
| activity_work | #22C55E1F | #4ADE8029 | the engine tint, as in themes/bolt.md |
| activity_need | #EAB30829 | #FACC152E | the engine tint, as in themes/bolt.md |
| activity_fail | #EF44441F | #F871712E | the engine tint, as in themes/bolt.md |
| shadow | #00000000 | #00000000 | flat: no glass |
| glass_highlight | #00000000 | #00000000 | flat: no glass |
| backdrop_1 | #00000000 | #00000000 | flat: no glass |
| backdrop_2 | #00000000 | #00000000 | flat: no glass |
| backdrop_3 | #00000000 | #00000000 | flat: no glass |

## Sizes

| token | value | use |
|---|---|---|
| space_1 | 2 | Dense |
| space_2 | 4 | Dense |
| space_3 | 8 | Dense |
| space_4 | 12 | Dense |
| radius | 0 | Square controls |
| text_sm | 11 | Labels |
| text_base | 13 | UI text |
| text_code | 12 | Editor lines and terminal output |
| text_lg | 14 | Titles stay small |
| pane_gap | 0 | Parts meet |
| pane_radius | 0 | Square panes |

## Font

| token | value | use |
|---|---|---|
| font | .SystemUIFont | The UI, like the editor's own |
| font_mono | Menlo | Code, line numbers, terminal |

## Scene

| setting | value | use |
|---|---|---|
| scene | none | No scene behind a workbench |
| motion | 0 | Nothing moves: the backdrop and the scene stay still |
