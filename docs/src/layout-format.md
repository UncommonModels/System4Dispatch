# Layout file format

A layout is a single JSON file. The editor's **Export…** and **Import…** buttons read and write it, and the app stores the applied layout in the same format. You can also write or edit one by hand, or generate one with a script. This chapter describes every field.

The format is defined by `crates/core/src/layout.rs`. The demo layout is [`crates/core/demo-layout.json`](https://github.com/UncommonModels/System4Dispatch/blob/main/crates/core/demo-layout.json) in the repository.

A machine-readable [JSON Schema](schema/layout-v1.schema.json) of the format is published alongside this book (generated from the code, and checked in CI), and the [`s4d_core::layout` API docs](api/s4d_core/layout/index.html) describe every type and field. To have an editor such as VS Code check a hand-written file, add `"$schema": "https://uncommonmodels.github.io/System4Dispatch/docs/schema/layout-v1.schema.json"` at its top level; the app ignores that field.

## Top level

```json
{
  "version": 1,
  "name": "Hilltown Subdivision",
  "grid": { "cols": 32, "rows": 12 },
  "blocks": [ ... ],
  "turnouts": [ ... ],
  "turntables": [ ... ],
  "elements": [ ... ]
}
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `version` | number | `1` | File format version. The current version is **1**. A file with a higher version is refused ("layout format version N is newer than this app supports") rather than misread. |
| `name` | string | `""` | The layout's name, shown in the top bar. |
| `grid` | object | `{"cols": 32, "rows": 16}` | Grid size in tiles. `cols` and `rows` are whole numbers. (The editor limits each to 4 to 400.) |
| `blocks` | array | `[]` | Detection blocks; see [Blocks](#blocks). |
| `turnouts` | array | `[]` | Turnouts; see [Turnouts](#turnouts). |
| `turntables` | array | `[]` | Turntables; see [Turntables](#turntables). Left out when empty. |
| `elements` | array | `[]` | Tiles drawn on the grid; see [Elements](#elements). |

Every top-level field is optional; a missing one takes its default. Unknown fields are ignored. Files written by the app are pretty-printed with two-space indentation, and optional fields that are empty are left out.

### Event IDs

Wherever an event is expected, write it as a string of eight hex bytes, for example `"05.01.01.01.22.00.00.10"`. Separators between bytes may be dots, dashes, colons, spaces or nothing, and hex digits may be upper or lower case; the app always writes dotted uppercase. An event field that is absent means "no event".

## Blocks

```json
{
  "id": "B1",
  "name": "West Main",
  "occupied_event": "05.01.01.01.22.00.00.10",
  "clear_event": "05.01.01.01.22.00.00.11"
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Identifier, referenced by tiles. Must be unique among **all blocks and turnouts**. The editor uses `B1`, `B2`, … but any string works. |
| `name` | string | no | Display name. If empty, the panel shows the ID. |
| `occupied_event` | event | no | Event produced when the block becomes occupied. |
| `clear_event` | event | no | Event produced when the block becomes clear. |

The dispatcher treats these as follows:

- A **PCER** (event report) of `occupied_event` makes the block *Occupied*; of `clear_event`, *Clear*.
- A **Producer Identified Valid** reply (to the startup query) for `occupied_event` means *Occupied*; **Invalid** means *Clear*. The opposite holds for `clear_event`.
- Until one of those arrives, or after the connection drops, the block is *Unknown*.

## Turnouts

```json
{
  "id": "T1",
  "name": "Hilltown West",
  "normal_event": "05.01.01.01.22.00.01.00",
  "reverse_event": "05.01.01.01.22.00.01.01",
  "feedback": {
    "normal_event": "05.01.01.01.22.00.02.00",
    "reverse_event": "05.01.01.01.22.00.02.01"
  },
  "linked": "T2",
  "linked_inverted": false
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Identifier, referenced by turnout tiles. Unique among all blocks and turnouts. |
| `name` | string | no | Display name. If empty, the panel shows the ID. |
| `normal_event` | event | no | Event the panel produces to throw the turnout to normal. |
| `reverse_event` | event | no | Event the panel produces to throw it to reverse. |
| `feedback` | object | no | Point-position feedback, with optional `normal_event` and `reverse_event`. The turnout counts as having feedback if either is set. |
| `linked` | string | no | ID of another turnout that is always thrown together with this one. |
| `linked_inverted` | boolean | no, default `false` | Throw the linked turnout to the **opposite** position. Only written when `true`. |

Without `normal_event` (or `reverse_event`) the panel can't throw the turnout in that direction; the button press is ignored.

### Linked turnouts

Links are **symmetric**: both turnouts name each other, and both carry the same `linked_inverted` value. The editor always writes them that way.

For hand-edited files, the app is forgiving: if only one side names the other, the link still works in both directions. When either turnout of a linked pair is thrown to position *P*, the partner is thrown to *P*, or to the opposite position if `linked_inverted` is true. Each turnout can have only one partner; links don't chain.

## Turntables

A turntable and its roundhouse are one entry here, drawn around a `turntable` tile. Its tracks can point in any direction; only the leads need to line up with the grid.

```json
{
  "id": "TT1",
  "name": "Hilltown Turntable",
  "size": 2,
  "cw_event": "05.01.01.01.22.00.02.00",
  "ccw_event": "05.01.01.01.22.00.02.01",
  "tracks": [
    { "angle": 75, "name": "Stall 1", "stall": true, "feedback_event": "05.01.01.01.22.00.02.10" },
    { "angle": 95, "name": "Stall 2", "stall": true },
    { "angle": 315, "name": "Lead" }
  ]
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Identifier, referenced by the `turntable` tile. Unique among all blocks, turnouts and turntables. The editor uses `TT1`, `TT2`, … |
| `name` | string | no | Display name. If empty, the panel shows the ID. |
| `size` | number | no, default `1` | Pit diameter in grid cells (1–8). The pit is centred on the tile and may cover neighbouring cells. |
| `cw_event` | event | no | Event the panel produces to turn the bridge to the **next track clockwise**. |
| `ccw_event` | event | no | Event the panel produces to turn it to the next track **counter-clockwise**. |
| `tracks` | array | no | The tracks around the pit, in clockwise order from north. These are the bridge's stops. |

Each track:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `angle` | number | yes | Direction from the pit's centre in whole degrees, clockwise from north (`0`–`359`), as drawn. Leads should be multiples of 45 (`270` = west) so they meet the grid's track. |
| `name` | string | no | Display name, such as "Stall 3". If empty, the angle ("75°") is shown. |
| `stall` | boolean | no, default `false` | A roundhouse stall: drawn running into the engine house. Otherwise the track is a lead that joins the rest of the panel. |
| `feedback_event` | event | no | Event the turntable controller produces when the bridge lines up with this track. |

The turntable works like a turnout with two directions: every `cw_event` moves the bridge one track clockwise and every `ccw_event` one track counter-clockwise, so the panel counts steps to know where the bridge is. To turn to a track it sends as many step events as it takes the short way round. Arriving at a track with a `feedback_event` is waited for (up to 90 seconds, then the turntable is flagged *no feedback*); a track without one is taken as reached. The panel learns the bridge's position from feedback events, or you can tell it on the panel.

The engine house is drawn as a faceted ring around all the stalls. Tracks must be in clockwise order with distinct angles; the editor keeps them that way. Rotating the `turntable` tile in the editor turns every track by 45°.

## Elements

Each element occupies one grid cell, at column `x` and row `y`, counted from 0 at the **top left**. There can be only one element per cell (the editor replaces whatever was there).

### Track tiles

```json
{ "type": "track", "x": 6, "y": 6, "shape": "turnout_left", "rot": 0, "block": "B2", "turnout": "T1" }
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | `"track"` | yes | `"turnout"` is accepted as a synonym when reading. |
| `x`, `y` | number | yes | Cell position. |
| `shape` | string | yes | One of the [shapes](#shapes) below. |
| `rot` | number | no, default `0` | Clockwise rotation in 45° steps, `0` to `7`. |
| `block` | string | no | ID of the block this tile belongs to. Track without a block is drawn as "no detection". |
| `block2` | string | no | For crossings, diamonds and slips: the block of the **other** track through the tile (the one that doesn't start at the W edge before rotation). If absent, both tracks are in `block`. |
| `turnout` | string | for turnout shapes | ID of the turnout this tile shows and throws. |
| `turnout2` | string | for `three_way`, `single_slip` and `double_slip` tiles | ID of the switch's **right-hand** turnout. (`turnout` is the left-hand one.) |
| `turntable` | string | for `turntable` tiles | ID of the turntable this tile shows. |
| `portal` | string | for `portal` tiles | The portal's name. The track carries on at the other portal with the same name. |

### Labels

```json
{ "type": "label", "x": 12, "y": 3, "text": "Hilltown" }
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | `"label"` | yes | |
| `x`, `y` | number | yes | Cell position. The text starts at the left of this cell and may extend to the right over other cells. |
| `text` | string | yes | The text to show. |

## Shapes and rotation

### Direction numbering

Track enters and leaves a tile at eight points: the middle of each edge and each corner. They are numbered clockwise from north (up):

```text
   7 ──── 0 ──── 1          0 = N    edge
   │             │          1 = NE   corner
   │             │          2 = E    edge
   6      ·      2          3 = SE   corner
   │             │          4 = S    edge
   │             │          5 = SW   corner
   5 ──── 4 ──── 3          6 = W    edge
                            7 = NW   corner
```

A track segment joins two of these points. A segment between opposite points (0–4, 1–5, 2–6, 3–7) is drawn straight; any other pair is drawn as a curve through the tile centre. A bumper's segment runs from one point to the centre, with a stop across it.

Neighbouring tiles connect where their points coincide: tile *(x, y)*'s E point (2) is tile *(x+1, y)*'s W point (6), and its NE corner (1) is tile *(x+1, y−1)*'s SW corner (5).

### Shapes

Unrotated (`rot` 0), every shape has a track end at the **west** edge (6):

| `shape` | Editor name | Segments at `rot` 0 | Normal leg | Reverse leg |
|---|---|---|---|---|
| `straight` | Straight | 6–2 (W–E) | | |
| `curve` | Curve | 6–1 (W–NE) | | |
| `tight_curve` | Tight curve | 6–0 (W–N) | | |
| `bumper` | Bumper | 6–centre | | |
| `crossing` | Crossing | 6–2 and 0–4 | | |
| `crossing_diag` | Diamond | 6–2 and 7–3 | | |
| `turnout_left` | Turnout L | | 6–2 (W–E) | 6–1 (W–NE) |
| `turnout_right` | Turnout R | | 6–2 (W–E) | 6–3 (W–SE) |
| `wye` | Wye | | 6–1 (W–NE) | 6–3 (W–SE) |
| `three_way` | 3-way | | 6–2 (W–E) | 6–1 (W–NE) or 6–3 (W–SE) |
| `single_slip` | Single slip | | 6–2 and 7–3 (the diamond) | 6–3 (slip road) |
| `double_slip` | Double slip | | 6–2 and 7–3 (the diamond) | 6–3 and 7–2 (slip roads) |
| `turntable` | Turntable | from its turntable's leads | | |
| `portal` | Portal | 6–centre, with an arrow | | |

`turnout_left`, `turnout_right` and `wye` are the **turnout shapes**: they have separate normal and reverse legs and need a `turnout`.

`three_way`, `single_slip` and `double_slip` are each worked by two turnouts, `turnout` (left-hand) and `turnout2` (right-hand). The positions of the pair choose the route:

| Shape | Normal, normal | Reverse, normal | Normal, reverse | Reverse, reverse |
|---|---|---|---|---|
| `three_way` | W–E | W–NE (left) | W–SE (right) | — |
| `single_slip` | W–E | W–SE (slip) | — | NW–SE |
| `double_slip` | W–E | W–SE (slip) | NW–E (slip) | NW–SE |

Slip roads are drawn bowed out beside the diamond so a slip can be told apart from a plain crossing. Clicking the switch on the panel steps through the routes in the order of the table.

A `turntable` tile draws its turntable's pit, tracks, roundhouse and bridge (see [Turntables](#turntables)). Its `rot` is always 0, because the geometry lives in the turntable's track angles.

A `portal` is a track end that carries on somewhere else, like the tunnel in Pac-Man that leaves one side of the screen and comes back on the other. The track enters from the W edge (before rotation) and the arrow points the way it leaves. Two portals with the same `portal` name are the two ends of one connection, so a line that runs off the east edge of the panel can come back on at the west edge, or a long branch can be drawn somewhere with more room.

Bends are drawn as two straight legs meeting at the tile centre, so curves look angular while keeping the 45° grid.

### Rotation

`rot` turns the whole tile clockwise by `rot` × 45°. Each direction *d* becomes (*d* + `rot`) mod 8; the centre stays the centre. For example:

| Tile | Segments |
|---|---|
| `straight`, `rot` 0 or 4 | W–E (horizontal) |
| `straight`, `rot` 1 or 5 | NW–SE (diagonal, top-left to bottom-right) |
| `straight`, `rot` 2 or 6 | N–S (vertical) |
| `straight`, `rot` 3 or 7 | NE–SW (diagonal, bottom-left to top-right) |
| `bumper`, `rot` 0 | W–centre: the line arrives from the left and stops |
| `bumper`, `rot` 4 | E–centre: the line arrives from the right and stops |
| `turnout_right`, `rot` 2 | normal N–S, reverse N–SW (points at the top) |
| `turnout_left`, `rot` 4 | normal E–W, reverse E–SW (points at the right) |

## Validation

The editor checks the draft continuously and lists any problems (see [Problems list](editor.md#problems-list)). These are the checks, with the messages they produce:

| Check | Message |
|---|---|
| Block, turnout and turntable IDs must all be distinct (a block and a turnout can't share an ID either) | `duplicate id B1` |
| `linked` must name an existing turnout | `turnout T1 is linked to unknown turnout T9` |
| A turnout can't be linked to itself | `turnout T1 is linked to itself` |
| If the partner names a different turnout, the link is inconsistent | `turnout T1 is linked to T2, but T2 is linked to T3` |
| Every element must be inside the grid (`x` < `cols`, `y` < `rows`) | `tile at 40,2 is outside the grid` |
| A tile's `block` must name an existing block | `tile at 6,6 refers to unknown block B9` |
| A tile's `turnout` must name an existing turnout | `tile at 6,6 refers to unknown turnout T9` |
| A turnout shape must have a `turnout` | `turnout tile at 6,6 has no turnout assigned` |
| A tile's `turntable` must name an existing turntable | `tile at 12,8 refers to unknown turntable TT9` |
| A `turntable` tile must have a `turntable` | `turntable tile at 12,8 has no turntable assigned` |
| A 3-way tile must have a `turnout2` (and it must exist) | `3-way tile at 4,2 has no right-hand turnout assigned` |
| Turntable tracks need distinct angles in `0..360`, in clockwise order | `turntable TT1 has two tracks at 75°` |
| A portal needs a name | `portal at 0,3 has no name` |
| Each portal name must be used by exactly two portals | `portal A has no other end` |

Problems are warnings: a layout with problems still loads and can be applied. Broken references simply do nothing (a tile pointing at an unknown block is drawn as "no detection"). Only malformed JSON, a wrong type, an invalid event ID, an unknown shape, or a too-new `version` prevent a file from loading at all.

## A complete example

A short main line with a siding, joined by a crossover whose two turnouts are linked. `T1` has point feedback; `T2` doesn't.

```text
   col:  1  2  3  4  5  6  7  8  9  10 11
row 1:   Mill Creek
row 2:               ⊣──T2───────⊢         B2 "Siding"
                       ╱
row 3:   ⊣───────────T1────────────────⊢   B1 "Main"
```

```json
{
  "version": 1,
  "name": "Mill Creek",
  "grid": { "cols": 13, "rows": 5 },
  "blocks": [
    {
      "id": "B1",
      "name": "Main",
      "occupied_event": "05.01.01.01.22.00.00.10",
      "clear_event": "05.01.01.01.22.00.00.11"
    },
    {
      "id": "B2",
      "name": "Siding",
      "occupied_event": "05.01.01.01.22.00.00.12",
      "clear_event": "05.01.01.01.22.00.00.13"
    }
  ],
  "turnouts": [
    {
      "id": "T1",
      "name": "Crossover West",
      "normal_event": "05.01.01.01.22.00.01.00",
      "reverse_event": "05.01.01.01.22.00.01.01",
      "feedback": {
        "normal_event": "05.01.01.01.22.00.02.00",
        "reverse_event": "05.01.01.01.22.00.02.01"
      },
      "linked": "T2"
    },
    {
      "id": "T2",
      "name": "Crossover East",
      "normal_event": "05.01.01.01.22.00.01.02",
      "reverse_event": "05.01.01.01.22.00.01.03",
      "linked": "T1"
    }
  ],
  "elements": [
    { "type": "label", "x": 1, "y": 1, "text": "Mill Creek" },

    { "type": "track", "x": 1,  "y": 3, "shape": "bumper",       "rot": 4, "block": "B1" },
    { "type": "track", "x": 2,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 3,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 4,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 5,  "y": 3, "shape": "turnout_left", "rot": 0, "block": "B1", "turnout": "T1" },
    { "type": "track", "x": 6,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 7,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 8,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 9,  "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 10, "y": 3, "shape": "straight",     "rot": 0, "block": "B1" },
    { "type": "track", "x": 11, "y": 3, "shape": "bumper",       "rot": 0, "block": "B1" },

    { "type": "track", "x": 5,  "y": 2, "shape": "bumper",       "rot": 4, "block": "B2" },
    { "type": "track", "x": 6,  "y": 2, "shape": "turnout_left", "rot": 4, "block": "B2", "turnout": "T2" },
    { "type": "track", "x": 7,  "y": 2, "shape": "straight",     "rot": 0, "block": "B2" },
    { "type": "track", "x": 8,  "y": 2, "shape": "straight",     "rot": 0, "block": "B2" },
    { "type": "track", "x": 9,  "y": 2, "shape": "bumper",       "rot": 0, "block": "B2" }
  ]
}
```

Things to notice:

- `T1` at (5, 3) is a `turnout_left` with its points facing west; its reverse leg leaves through the NE corner. `T2` at (6, 2) is a `turnout_left` rotated 180° (`rot` 4), so its points face east and its reverse leg leaves through the SW corner. The two corners meet, forming the crossover.
- `T1` and `T2` name each other in `linked`, so throwing either sends both. `linked_inverted` is absent, so they go to the same position: both normal (main and siding straight through) or both reverse (crossing over).
- The bumpers at (1, 3) and (5, 2) use `rot` 4 so their track comes in from the right; the ones at (9, 2) and (11, 3) use `rot` 0.
- The label sits in a cell of its own, and its text runs to the right.
