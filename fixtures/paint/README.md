# Paint fixtures

Saved answers for the terminal painter: input frames, and the display list
the web painter (`app/src/painter/plan.ts`) makes for each. Any other
painter (the phone app's) must make the same lists from the same inputs.

**Don't edit by hand.** `npm run gen:paint` (in `app/`) rewrites
everything here; `npm run check:paint`, and plain `npm test`, fail when
the web painter draws something these files don't say. So a painter
change that alters what's drawn shows up in the diff of `expected/`.

## Files

- `metrics.json`: three metric sets, by name. All in device pixels,
  integers:
  - `1x`: 14px at scale 1;
  - `2x`: 14px at scale 2;
  - `odd`: 13px at scale 1.5, where cells and lines don't divide evenly.
- `inputs/<name>.jsonl`: one frame per line, in the wire format
  (`crates/proto`, `Frame`), in order. Later frames may be partial.
  - `box`: every box-drawing, block, braille and Powerline character.
  - `styles`: colors, bold/italic, faint, invisible, every decoration.
  - `wide`: wide and composed graphemes, one that doesn't fit at the edge.
  - `cursor`: one full frame, then cursor-only frames (every shape, wide
    heads and tails, box characters, the last column, hidden, out of view).
  - `man`, `sync`, `vim`: real programs (from `app/src/dev/recordings`).
- `expected/<name>.<metrics>.jsonl`: the answers. The synthetic inputs at
  all three metric sets; the recordings at `1x` only.

## Answer lines

Apply the frames in order to a grid mirror (`app/src/grid.ts`), and after
each one write:

1. `{"seq", "applied": false, "requestFull"}` if the frame was dropped (a
   gap in `seq`, or a partial frame first); nothing else for that frame.
2. Otherwise `{"seq", "applied": true, "cols", "rows"}`, then
3. one `{"seq", "row", "ops"}` per row painted, in ascending order:
   every row after a full or resizing frame, else only the changed rows;
4. then `{"seq", "cursor"}`: the cursor's ops, focused with blink on, or
   `null` when it's hidden or out of view. A cursor on a wide
   character's tail is drawn on its head (`cursorColumn`).

`ops` is the display list, painted in order (`PaintOp` in `plan.ts`).
Positions and sizes are device pixels; colors are `0xRRGGBB` as numbers.

| `k` | Fields | Meaning |
|---|---|---|
| `fill` | `x y w h color alpha` | A filled rectangle: backgrounds, straight decorations, cursors |
| `glyph` | `x y text cells font fg alpha` | A grapheme across `cells` cells (1 or 2) from `x`, row top `y`. `font`: 0 regular, 1 bold, 2 italic, 3 bold italic. Placing it inside the cells (baseline, centering, clipping) is the drawing side's |
| `box` | `x y w h ops fg bg alpha` | A box-drawing character as geometry; `ops` are relative to the cell (`boxdraw.ts`'s `Op`: `rect` with optional `bg: true` for double-line gaps and `a` for shade alpha, `fill` polygons, `stroke` paths with `M`/`L`/`A` arcs) |
| `wave` | `x y w h mid amp cellW lw color alpha` | A curly underline: one wave per cell from `x` across `w`, centered on `mid`, clipped to `x y w h` |
| `frame` | `x y w h lw color alpha` | An outline (the hollow cursor), the line inside the edges |

## Comparing

Compare structurally, op by op. Most numbers are integers; arcs carry
angles like `1.5707963267948966` and waves can have `amp: 1.5`, so
compare numbers with a tolerance of `1e-9` rather than as text. Report the
first difference as file, line, seq, row (or cursor) and op index, as
`app/src/painter/fixtures.test.ts` does.
