# Terminal

A phosphor screen at rest, behind the glass. A block cursor wanders the
window, types a short line of glowing traces (word-shaped bars, not
letters), pauses blinking, and jumps somewhere else; old lines fade like
phosphor, and a faint scanline sweeps slowly down the whole window. Some
lines are amber, like a prompt. Nothing falls.

The behaviour is code (`crates/core/src/scene/terminal.rs`); it needs no
sprites. Its colours are its own, phosphor green and amber in light and
dark, whatever the theme.

## Scene

| key | value | use |
|---|---|---|
| kind | terminal | The behaviour that moves it |
| place | window | The whole background, behind the panes |
| fps | 24 | A film's rate suits a terminal; at 30 or 60 the traces glow and the scanline gets a soft tail |
| cell | 3 | Logical pixels per cell |

## Palette

| key | role | light | dark | use |
|---|---|---|---|---|
| t | - | #0A6B1F | #33FF66 | Typed traces (phosphor) |
| c | - | #000000 | #C8FFD6 | The cursor (hot phosphor) |
| l | - | #0A6B1F | #33FF66 | The scanline sweep |
| a | - | #9A6A00 | #FFB000 | Amber prompt lines |
