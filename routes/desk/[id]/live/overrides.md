# Overrides

The live view of an engine's developer artifacts, drawn as the code editor a person knows: the Code page's dense, square, flat metrics (`routes/code/overrides.md`, copied so this folder stands alone) and the editor's default themes, Light Modern in light mode and Dark Modern in dark (MIT-licensed theme-defaults of the open-source code editor), so it follows the app's mode instead of a picker.

## Colors

| token | light | dark | from |
|---|---|---|---|
| background_body | #FFFFFF | #1F1F1F | editor.background |
| background_surface | #F8F8F8 | #181818 | sideBar.background / panel.background |
| background_muted | #FFFFFF | #313131 | input.background |
| overlay_hover | #F2F2F2 | #2A2D2E | list.hoverBackground |
| overlay_pressed | #E4E6F1 | #37373D | list.inactiveSelectionBackground |
| text_primary | #3B3B3B | #CCCCCC | editor.foreground |
| text_secondary | #616161 | #9D9D9D | foreground / descriptionForeground |
| text_disabled | #6E7681 | #6E7681 | editorLineNumber.foreground |
| text_accent | #1F1F1F | #D7D7D7 | activityBar.foreground |
| accent | #005FB8 | #0078D4 | focusBorder / button.background |
| accent_muted | #E4E6F1 | #04395E | list.activeSelectionBackground |
| on_accent | #FFFFFF | #FFFFFF | button.foreground |
| border | #E5E5E5 | #2B2B2B | panel.border |
| border_emphasized | #CECECE | #3C3C3C | input.border |
| error | #F85149 | #F85149 | errorForeground |
| window_tint | #F8F8F8 | #181818 | activityBar.background |
| glass_fill | #F8F8F8 | #181818 | sideBar.background (flat: no glass) |
| glass_sheen | #F8F8F8 | #181818 | sideBar.background (flat: no glass) |
| glass_edge | #E5E5E5 | #2B2B2B | sideBar.border |
| status_bar | #F8F8F8 | #181818 | statusBar.background |
| on_status_bar | #3B3B3B | #CCCCCC | statusBar.foreground |
| line_highlight | #EEEEEE | #282828 | editor.lineHighlightBorder |
| activity_llm | #005FB81F | #0078D433 | button.background, tinted |
| activity_tools | #AF00DB1A | #C586C033 | symbolIcon.functionForeground, tinted |
| activity_compress | #0F7B6C1A | #4EC9B033 | symbolIcon.classForeground, tinted |
| activity_wait | #BF88031F | #CCA70033 | editorWarning.foreground, tinted |
| activity_work | #388A341A | #89D18533 | testing.iconPassed, tinted |
| activity_need | #BF88032E | #CCA70047 | list.warningForeground, tinted |
| activity_fail | #E514001F | #F8514933 | errorForeground, tinted |
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
| motion | 0 | Nothing moves: the backdrop and the scene stay still |
