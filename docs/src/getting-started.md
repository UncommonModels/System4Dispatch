# Getting started

## Step 1: try the simulator

The first time you open System4Dispatch, it starts with the **Simulator** connection and a demo layout, the *Hilltown Subdivision*. It connects automatically, so within a fraction of a second the status pill in the top bar should read **Online** followed by a three-character code (that code is the panel's short address on the bus; see [Node IDs and aliases](connecting.md#node-ids-and-aliases)).

The simulator behaves like a small real layout:

- a **layout controller** with an occupancy detector for every block and a turnout driver (with point-position feedback) for every turnout, and
- a **command station** that will hand you any DCC address you ask for.

Things to try:

1. **Watch the blocks.** Every three seconds the simulator picks a block at random and flips it between clear and occupied. Occupied track turns red on the diagram and in the block list.
2. **Throw a turnout.** Tap a turnout on the diagram, or press **N** or **R** next to it in the Turnouts list. The leg the points are moving to blinks for a moment, until the simulated point sensor reports back (about 0.4 s later). Then the new route is drawn bright and the old one faded.
3. **Run a train.** Open **Throttles**, leave the address as `3` (or type any address from 1 to 10239), and press **Acquire**. The card goes from *Searching…* to *Taking control…* to *In control*. Drag the slider, change direction, and toggle functions. Nothing moves, of course, but every command goes through the same protocol a real command station would see.
4. **Look at the traffic.** In **Settings**, tick *Show bus monitor on the panel* and press **Save settings**. The panel now shows each message sent (→) and received (←), decoded. The same monitor also appears at the bottom of the Settings tab.
5. **Open the editor.** The demo layout is fully editable. Change something, press **Apply & save**, and the simulator rebuilds itself around your new layout.

Everything you do is saved automatically: your settings as soon as you press **Save settings**, and your layout when you press **Apply & save** in the editor.

## Step 2: draw your own layout

When you're ready, open the **Editor**. You can start from the demo (press **Demo**) or from an empty grid (press **New**). The short version:

1. Add one **block** per detection section in the Blocks table (**+ Add block**) and give each a name.
2. Choose a block under **Paint block**, choose a track piece from the palette, and click or drag on the grid.
3. Placing a **Turnout L**, **Turnout R** or **Wye** creates a turnout entry for it.
4. Fill in the event IDs (or learn them from the bus later) and press **Apply & save**.

You can keep using the simulator while you draw: it always models whatever layout is currently applied, with a detector for every block and a driver for every turnout. [The layout editor](editor.md) chapter covers all of this in detail.

## Step 3: connect to real hardware

You need two things on the layout side:

- **An LCC bus** with your devices on it: occupancy detectors, turnout drivers (stall-motor or servo controllers), fascia buttons, and for throttles, an LCC command station that supports the OpenLCB **Traction** protocol.
- **A way for the computer to reach the bus.** Either a **USB LCC adapter** that speaks GridConnect (for example an RR-CirKits LCC Buffer-USB, a TCS USB adapter, or a SPROG CANISB), or a **GridConnect TCP hub** on the network (JMRI's LCC hub, an Ethernet LCC buffer, or `rolcb-hub`).

Then:

1. Open **Settings** and choose a connection under **Connection**:
   - **USB serial (GridConnect)**: in the web app, just press **Connect** in the top bar and your browser will ask which port to use. In the desktop app, choose the port from the list first.
   - **Network (GridConnect TCP)** (desktop app only): enter the hub's address as `host:port`, for example `jmri.local:12021`.
2. Press **Save settings**, then **Connect** if it didn't connect by itself.
3. Wait for **Online** in the top bar.

The panel then asks every device on the bus for the current state of each block and turnout, so it is up to date straight away rather than waiting for something to change. If some blocks stay grey (*Unknown*), no device answered for their events; see [Troubleshooting](troubleshooting.md).

[Connecting to your layout](connecting.md) covers each connection type, sharing one adapter between programs, and Node IDs.

## Moving a layout between computers

The layout lives in the browser (web app) or your settings folder (desktop app), so it doesn't follow you automatically. In the editor, press **Save to disk…** to save it as a `.json` file, and **Import…** on the other computer to load it. Import loads the file as a draft; press **Apply & save** to keep it.
