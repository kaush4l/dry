# Abyss

The deep sea, lit from inside. Jellyfish rise through the dark behind the
glass: each bell contracts and relaxes, and every contraction is a surge
upward, so they climb in pulses, trailing swaying tentacles, cyan and
magenta in turn. Plankton motes drift up and twinkle; faint shafts of
light lean in from far above.

The behaviour is code (`crates/core/src/scene/abyss.rs`); the bell is
here. Its colours are its own (light and dark), whatever the theme; only
the bell's inside takes the theme's `accent_muted`.

## Scene

| key | value | use |
|---|---|---|
| kind | abyss | The behaviour that moves it |
| place | window | The whole background, behind the panes |
| fps | 30 | 30 buys glowing bells, 70 motes and smoother shafts; 60 buys the in-between bell frames, 120 motes and longer tentacles. Never faster |
| cell | 4 | Logical pixels per cell |

## Palette

| key | role | light | dark | use |
|---|---|---|---|---|
| j | - | #00A9C0 | #3FF8FF | Cyan glow: bell rims, tentacles |
| m | - | #D0309F | #FF4FD8 | Magenta glow: spots (the other jellyfish swap the two) |
| i | accent_muted | #008C9E2E | #18D6E633 | Inside the bell: the theme's muted accent |
| p | - | #0E6E80 | #B8FFF4 | Plankton motes |
| s | - | #FFFFFF | #7FD8FF | Light shafts |

## Sprites

The bell: relaxed, contracting, contracted, relaxing. Below 60 fps only
relaxed and contracted show.

### bell

```
...jjjjj...
.jjiiiiijj.
jiimiiimiij
jiiiiiiiiij
j.j.j.j.j.j
...........
...........
```

```
....jjj....
..jjiiijj..
.jiimimiij.
.jiiiiiiij.
..jiiiiij..
..j.j.j.j..
...........
```

```
....jjj....
...jiiij...
...jimij...
...jiiij...
...jimij...
...jiiij...
...j.j.j...
```

```
...jjjjj...
..jiiiiij..
.jiimimiij.
.jiiiiiiij.
.j.j.j.j.j.
...........
...........
```
