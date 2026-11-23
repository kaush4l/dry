# Cat

dry's own cat, the one from the first app (`app/eggs/pet` in dry), with
the same body and the same ways. A small round cat lives on the floor above
the footer. It runs under your pointer, jumps for it when it is close,
stalks and pounces on a pointer held still, purrs hearts when you click
it, and naps when left alone.

The body is the first cat's: drawn from parts (legs, tail, body, ears,
head), each one twice, first fattened in the outline colour and then
filled, so the whole cat gets one sticker outline. The parts are paths
(ellipses, round strokes along lines and curves, the ear triangles); the
core fills them into a pixel grid and the renderers paint its runs. The
part sizes and every pose are numbers in the behaviour
(`crates/core/src/scene/cat.rs`, `body`), as they were in `egg.js`.

What it does (the full list is in `docs/scenes.md`):

- **Chase:** it runs under the pointer, speed by distance (up to 480 px/s,
  1500 px/s² to get there), after a beat to notice (260 ms) if the pointer
  was quiet a while.
- **Jump:** a pointer just above its head (50-340 px over the floor):
  it crouches (230 ms) and jumps in an arc (gravity 2600 px/s²), paws
  reaching for it, the spine arched most at the top, hind legs tucked; it
  lands with a squash and a damped follow-through in the tail and head.
- **Misses:** too high, it jumps anyway (reach 70 px, +40 px each miss, up
  to 3 misses; three tries a spot) and gets more excited.
- **Pounce:** held still in reach 2.6 s: it stalks (low, wiggling, tail
  flicking), pounces (a quicker 120 ms crouch), catches it, and hearts burst.
- **Purr:** click it and it purrs: a wiggle and hearts that pop in, drift
  and wobble as they rise, in varied sizes.
- **Nap:** 20 s with no pointer and it falls asleep (lying down, eyes
  shut, `z z`); any move wakes it.
- **Edges:** the pointer leaving through a side sends it to that edge.
- **Rest:** sitting still it draws nothing; now and then it blinks or
  swishes its tail.
- **Still** (`motion` 0, reduced motion): it sits and only turns its head
  to follow you.

From the engines (`dry_core::scene::mood`, Home's tiles' rule), two
things that do not change any of the above: **needs you** shows a `!` over
its head, and **done** (working, then idle) makes it purr, as a click does.

Its colours are its own, with roles: each key names the theme token
the first app used for it (`--dry-accent`, `--dry-accent-ink`,
`--dry-accent-text`, `--dry-danger`: `accent`, `on_accent`, `text_accent`
and `error`), so the cat wears the highlight colour of whatever theme is
on (terracotta on the cat theme, blue on the default one). A theme without
the role draws the light or dark colour here (the cat theme's own values).

## Scene

| key | value | use |
|---|---|---|
| kind | cat | The behaviour that moves it |
| place | front | window: the whole background, behind the panes; front: the whole window over the panes (clicks pass through), so their frost does not hide the cat; footer: the footer row (always 24 fps) |
| fps | 60 | At most this many frames a second; 60 buys the fine pixel grid, never speed |
| cell | 1 | Logical pixels per cell: 1, because every size, speed and jump of the cat is in CSS pixels, as in dry |
| pixel | 1 | Logical pixels per pixel of the grid the cat is filled into (0.5 is finer and draws more rectangles); below 60 fps the grid is twice as coarse |

## Palette

| key | role | light | dark | use |
|---|---|---|---|---|
| f | accent | #D9733F | #E8875A | Fur; the eyes' light; the `z z` |
| o | on_accent | #FFFFFF | #2A1A10 | The sticker outline round every part, and the eyes |
| d | text_accent | #B4532A | #FFB37A | The far legs and the tail |
| h | error | #C8323C | #FF6B6B | The nose and the hearts |
| c | accent | #D9733F | #E8875A | The `!` when the engines need you |

## Body

The parts, back to front, as `egg.js` draws them (px; the cat faces
right, mirrored to face left; y is up from the floor). `s` is how much
it sits (0-1), `c` how much it crouches.

| part | shape | colour |
|---|---|---|
| far front leg | a 5 px stroke from the shoulder to the paw | d |
| far hind leg | a 5 px curved stroke from the haunch (only when not sitting) | d |
| tail | a 4.5 px curve from the haunch; sways after it settles, flicks while stalking, trails in the air | d |
| far ear | a triangle on the head, folded back as the ears drop | d |
| body | a 17 px stroke (less 1.5 crouching) along a curve haunch to shoulder, arched in the air | f |
| haunch | sitting: a 10 x 8.5 ellipse and a 5 px foot along the floor; else the near hind leg | f |
| near front leg | a 5 px stroke; in the air both front legs reach for the pointer and end in 3.5 px paws | f |
| head | a 10.5 x 9.5 ellipse, a 5.5 x 4.2 muzzle, the near ear; tilts to look at the pointer | f |
| eyes | 1.9 x 2.6 ovals with a light (a 0.4 slit when blinking), arcs when happy (up) or asleep (down) | o, f |
| nose | a 1.3 px dot | h |

Every part is first drawn 2 px fatter (strokes 4 px wider) in `o`, then
filled.

## Sprites

The two that are not the body. `.` is clear.

### sleep

The `z z` over a sleeping cat.

```
ffffff...ffffff
....ff......ff.
...ff......ff..
..ff......ff...
.ff......ff....
ffffff...ffffff
```

### cue

The `!` over its head when the engines need you.

```
.ooo.
occco
occco
occco
occco
occco
.oco.
.oco.
.ooo.
.....
.ooo.
occco
.ooo.
```
