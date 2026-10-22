# The layout editor

The **Editor** tab is where you draw your panel and tell it which LCC events belong to each block and turnout. A layout is a grid of square **tiles**. Each track tile has a shape and a rotation, and can belong to a **block** (a detection section) and, for turnout shapes, a **turnout**.

The screen has four parts:

- the **toolbar** at the top: the tools and track pieces, rotation, the block to paint with, and display options;
- the **grid** in the middle;
- the **inspector** on the right: layout name and size, save/load buttons, any problems found, and details of the selected tiles;
- the **Blocks** and **Turnouts** tables underneath.

## Drafts: Apply & save, Revert

Everything you change in the editor goes into a **draft**. The panel, the simulator and the bus keep using the last applied layout until you press **Apply & save**. Then the draft becomes the live layout, is saved (in the browser, or your settings folder on desktop), and the panel picks up any changes. Block and turnout states are kept for IDs that still exist, and the panel asks the bus for the state of any new events.

**Revert** throws the draft away and goes back to the live layout. Both buttons are greyed out when there are no unapplied changes.

The draft survives switching tabs, so you can flip to the panel and back, but it is not saved anywhere until you apply it. Reloading the page or closing the app loses unapplied changes.

If you're using the simulator, applying rebuilds it around the new layout, with every block clear and every turnout normal.

## The layout box

At the top of the inspector:

- **Name**: shown in the top bar under "System4Dispatch", and used as the file name when you export.
- **Columns** and **Rows**: the grid size, from 4 to 200 each. Shrinking the grid doesn't delete tiles that fall outside it, but they are reported as problems (see below).
- **Apply & save** and **Revert**: see above.
- **Export…** saves the draft (or the live layout, if there's no draft) as a `.json` file: a download in the web app, a "Save as" dialog on desktop.
- **Import…** loads a `.json` layout file into the draft. Press Apply & save to use it. Files from a newer, incompatible version of the app are refused with an explanation.
- **Demo** loads the built-in Hilltown Subdivision demo into the draft.
- **New** starts an empty layout (after asking).

### Problems list

Under those buttons, the editor lists anything wrong with the draft, for example:

- `tile at 6,6 refers to unknown block B9`
- `turnout tile at 12,4 has no turnout assigned`
- `duplicate id B3`
- `turnout T1 is linked to T2, but T2 is linked to T3`
- `tile at 40,2 is outside the grid`

The list is a warning, not a lock: you can still apply a layout with problems, but things like an unassigned turnout tile won't do anything on the panel. The full set of checks is in the [file format reference](layout-format.md#validation).

## Tools

The toolbar's first row holds the tools:

| Tool | What it does |
|---|---|
| **☝ Select** | Click a tile to select it, drag a box to select several, Shift- or Ctrl-click (⌘-click on a Mac) to add or remove tiles. See [Selecting several tiles](#selecting-several-tiles). |
| **Straight**, **Curve**, **Bumper**, **Crossing**, **Diamond**, **Turnout L**, **Turnout R**, **Wye** | Place that track piece. Click a cell, or hold the button down and drag to lay a run of track. Placing on a cell that already has something replaces it. |
| **T Label** | Click a cell to place a text label (initially "Label"). The editor switches to Select with the new label selected so you can type its text in the inspector. |
| **⌫ Erase** | Click or drag over cells to remove whatever is there. |

The track pieces, before rotation:

| Piece | Shape |
|---|---|
| Straight | straight through, left edge to right edge |
| Curve | a gentle 45° curve, from the left edge to the top-right corner |
| Bumper | a track end with a buffer stop in the middle of the tile |
| Crossing | two straight tracks crossing at right angles |
| Diamond | a straight track crossed by a diagonal one (a 45° diamond crossing) |
| Turnout L | points on the left; normal goes straight on, reverse diverges up to the top-right corner |
| Turnout R | the same, diverging down to the bottom-right corner |
| Wye | points on the left; normal goes to the top-right corner, reverse to the bottom-right |

The palette icons show each piece at the current rotation.

The toolbar's second row has:

- **⟳ Rotate (0°)**: turns new pieces clockwise by 45° each press, through all eight directions. It affects tiles you place from now on; to turn a tile that's already placed, select it and use the ⟲ ⟳ buttons in the inspector.
- **Paint block**: new track pieces are put into this block. Leave it on "— none —" to lay track without a block. The swatch next to it shows the block's colour.
- **⊕ Multi** (only shown with the Select tool): see [Selecting several tiles](#selecting-several-tiles).
- **Block tags**: show or hide the block name tags on the drawing.

### Tiles connect at edges and corners

Track leaves a tile either through the **middle of an edge** (north, east, south, west) or through a **corner** (north-east, south-east, south-west, north-west). Two tiles join up when their track meets at the same point. Horizontal and vertical runs join edge to edge; diagonal runs join corner to corner, running through tiles that are diagonally adjacent. A Curve is how you get from one to the other: at rotation 0 it enters from the west edge and leaves through the north-east corner, so the next tile is up and to the right, a Straight rotated to run diagonally from bottom-left to top-right (135°, or 315°).

## Blocks

A **block** is a section of track with its own occupancy detector. On the panel, every tile in a block turns red together when that block's detector reports a train.

### The Blocks table

| Column | |
|---|---|
| (swatch) | The block's colour. Click it to **focus** that block (see below). |
| **ID** | A short identifier such as `B1`, assigned when you add the block. Tiles refer to blocks by ID. |
| **Name** | What the panel shows, for example "West Main". |
| **Tiles** | How many tiles are in this block. Click it (**3 ⬚**) to **select all of this block's tiles**. |
| **Occupied event** | The event the detector sends when the block becomes occupied. |
| **Clear event** | The event it sends when the block becomes clear. |
| ✕ | Delete the block. Its tiles stay but are no longer in any block. |

**+ Add block** adds a block with the next free ID (`B1`, `B2`, …). A note above the table tells you how many track tiles aren't in any block yet.

### Painting blocks

There are three ways to put track into a block:

1. **While drawing:** choose the block under **Paint block** before you lay track.
2. **One tile:** select it and pick the block from the **Block** list in the inspector.
3. **Many tiles at once:** select them (drag a box, or Shift/Ctrl-click) and pick the block in the inspector. If the selection spans several blocks, the list shows "— mixed —" until you choose.

### Block colours and tags

In the editor, track isn't coloured by occupancy. Instead **every block is drawn in its own colour**, and track that isn't in any block is drawn thin and grey. A small tag with the block's ID sits on the tile nearest the middle of each block. Untick **Block tags** to hide the tags.

Colours are assigned automatically, spread around the colour wheel so that neighbouring blocks in the list look clearly different. They depend on the block's position in the list, so deleting a block can change the colours of the ones after it. They appear only in the editor; the panel always shows occupancy colours.

### Focus and select-by-block

- **Focus:** click a block's swatch in the Blocks table to dim every other block on the drawing, so you can see exactly which tiles belong to it. Its row is highlighted. Click the swatch again, or **Show all blocks**, to go back.
- **Select by block:** click the tile count in a block's row to select all of its tiles (the editor switches to the Select tool). From there you can move the whole block, rotate it, delete it, or reassign it to another block.

## Turnouts

### The Turnouts table

| Column | |
|---|---|
| **ID** | A short identifier such as `T1`. Turnout tiles refer to turnouts by ID. |
| **Name** | What the panel shows, for example "Hilltown West". |
| **Throw normal** | The event the panel sends to throw the turnout to normal. Your turnout driver consumes it. |
| **Throw reverse** | The event the panel sends to throw it to reverse. |
| **Feedback normal** | Optional: the event a point sensor sends when the points arrive at normal. |
| **Feedback reverse** | Optional: the event it sends when they arrive at reverse. |
| **Thrown with** | Another turnout to link to, or "— alone —". See [Linking turnouts](#linking-turnouts). |
| ✕ | Delete the turnout. Tiles that used it become unassigned. |

Placing a **Turnout L**, **Turnout R** or **Wye** tile automatically adds a new turnout with the next free ID and assigns it to the tile, and selects the tile so you can see it in the inspector. Re-placing a different turnout shape over an existing turnout tile keeps its turnout. If you drag a turnout piece across several cells, each cell gets its own new turnout. **+ Add turnout** adds one by hand, and a turnout tile's **Turnout** list in the inspector lets you reassign it.

Leave both feedback events empty for turnouts without position sensors. The panel then shows the last command seen on the bus instead of waiting for confirmation. See [Feedback and pending throws](panel.md#feedback-and-pending-throws).

### Linking turnouts

To make two turnouts always move together (for example both ends of a crossover), pick the second one under **Thrown with** in the first one's row. The link is two-way: the other turnout's row now shows the first. Tick **opposite** if the second turnout should go to the opposite position.

Linking a turnout to a new partner automatically unlinks any previous partner of either turnout, and choosing "— alone —" removes the link from both. A turnout tile's inspector also shows "Thrown together with T2" (plus "(opposite position)" when applicable).

## Selecting several tiles

With the **☝ Select** tool:

- **Click** a tile to select just that tile. Clicking an empty cell clears the selection.
- **Drag a box** from one cell to another to select every tile inside it.
- **Shift-click**, **Ctrl-click** or **⌘-click** to add a tile to the selection, or remove it if it's already selected. Hold the key while dragging to add a box to the existing selection.
- On a touch screen, where there's no Shift key, turn on **⊕ Multi**. Then every tap adds or removes a tile, and dragging adds a box.
- **Escape** clears the selection.

Selected tiles are outlined. The inspector shows details for a single tile, or bulk actions for several.

### The inspector: one tile

For a **track tile**: its shape and position (for example "Turnout L at 6,6"), a **Shape** list to change it in place, **⟲** and **⟳** to rotate it by 45° (the current angle is shown between them), its **Block**, and for turnout shapes, its **Turnout**. **Delete tile** removes it.

For a **label**: a **Text** box and a **Delete** button. Labels are drawn starting at the left of their cell and may run over the cells to the right.

### The inspector: several tiles

With more than one tile selected, the inspector shows "*n* tiles selected" and offers:

- **Block**: put every selected track tile in one block (or none).
- **Rotate each ⟲ ⟳**: rotate every selected tile 45° about its own centre. This turns each piece in place; it doesn't rotate the group as a whole.
- **Clear selection**.
- **Delete *n* tiles**.

### Moving tiles

Whenever something is selected, a **Move** box appears with **↑ ← → ↓** buttons that shift the selection one cell at a time. You can also use the **arrow keys**, and **Delete** or **Backspace** to delete the selection. Keyboard shortcuts work when the grid has keyboard focus: click on the grid first.

A move is refused, with the message "Can't move there: tiles would leave the grid or overlap other track", if any tile would go off the grid or land on a tile that isn't part of the selection.

## Learning event IDs from the bus

You rarely need to type an event ID. Instead, let the layout tell you:

1. Connect to the layout (USB or network) so the panel is **Online**.
2. Make the device send its event: press the fascia button, throw the turnout with its local control, or roll a car into the block (and out again, for the clear event).
3. The event appears in the inspector under **Recent bus events**, newest first (the last eight distinct events reported on the bus are kept).
4. Every event field has a **⤓** button next to it. Press it to fill that field with the **newest** recent event. Hover over the button to see which event it will use.

Work through one field at a time: trigger the device, press ⤓ on the right field, then move on to the next. Only events that are *reported* (sent as they happen) show up; replies to the panel's own startup questions don't.

This works with the simulator too: its random block changes appear under Recent bus events.

### Typing event IDs

An event ID is eight bytes, written as hex: `05.01.01.01.22.00.00.10`. The editor also accepts dashes, colons or spaces between the bytes, or none at all (`0501010122000010`), and always shows them with dots. Letters can be upper or lower case. Clearing a field removes the event. If what you type isn't a valid event ID, the editor shows an error and keeps the old value.

You can find a device's events in its configuration tool, such as JMRI's *LCC Configure Nodes* window, or in the manufacturer's documentation.
