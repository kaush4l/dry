# Overrides

Token changes for this page only, applied while it is shown and taken off when you leave (see `pages::Local`). The same tables as `themes/*.md`, with only the rows that change; a theme from `themes/` here is laid on top.

Code wants to feel like a code editor: dense spacing, the system font at 13 for the UI and a monospace font at 12 for code, square corners, and no glass (no shadows, no light behind the panes).

## Colors

| token | light | dark | use |
|---|---|---|---|
| shadow | #00000000 | #00000000 | No pane shadows |
| glass_highlight | #00000000 | #00000000 | No light on pane edges |
| backdrop_1 | #00000000 | #00000000 | No backdrop light |
| backdrop_2 | #00000000 | #00000000 | No backdrop light |
| backdrop_3 | #00000000 | #00000000 | No backdrop light |

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
