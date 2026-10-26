# Getting started

## Step 1: try the simulator

The first time you open System4Dispatch, it shows a demo layout, the *Hilltown Subdivision*, in the Panel window. It doesn't connect to anything until you ask. To run the demo on the built-in simulator:

1. Press **Settings** in the dock at the bottom.
2. Under **Connection**, choose **Simulator**.
3. Press **Save settings**, then **Connect** in the top bar.

Within a fraction of a second the status pill in the top bar should read **Online** followed by a three-character code (that code is the panel's short address on the bus; see [Node IDs and aliases](connecting.md#node-ids-and-aliases)). The simulator always waits for you to press **Connect**, even if *Connect automatically when the app opens* is ticked.

The simulator behaves like a small real layout:

- a **layout controller** with an occupancy detector for every block and a turnout driver (with point-position feedback) for every turnout, and
- a **command station** that will hand you any DCC address you ask for.

Things to try:

1. **Watch the blocks.** Every three seconds the simulator picks a block at random and flips it between clear and occupied. Occupied track turns red on the diagram and in the block list.
2. **Throw a turnout.** Tap a turnout on the diagram, or press **N** or **R** next to it in the Turnouts list. The leg the points are moving to blinks for a moment, until the simulated point sensor reports back (about 0.4 s later). Then the new route is drawn bright and the old one faded.
3. **Run a train.** Press **+ Throttle** in the dock, leave the address as `3` (or type any address from 1 to 10239), and press **Acquire**. The throttle goes from *Searching…* to *Taking control…* to *In control*. Drag the slider, change direction, and toggle functions. Nothing moves, of course, but every command goes through the same protocol a real command station would see.
4. **Look at the traffic.** Press **Monitor** in the dock. The Monitor window shows each message sent (→) and received (←), decoded.
5. **Configure a node.** Press **Nodes** in the dock, then **Configure** next to the simulated layout controller. You can change how often blocks flip, or turn random occupancy off. See [Configuring LCC nodes](configuring-nodes.md).
6. **Open the editor.** The demo layout is fully editable. Change something, press **Apply & save**, and the simulator rebuilds itself around your new layout.

Everything you do is saved automatically: your settings as soon as you press **Save settings**, and your layout when you press **Apply & save** in the editor.

## Step 2: draw your own layout

When you're ready, press **Editor** in the dock. You can start from the demo (press **Demo**) or from an empty grid (press **New**). The short version:

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

1. Open **Settings** and choose a connection under **Connection**. A new install starts on **Network (GridConnect TCP)** in the desktop app and **USB serial (GridConnect)** in the web app.
   - **USB serial (GridConnect)**: in the web app, just press **Connect** in the top bar and your browser will ask which port to use. In the desktop app, choose the port from the list first.
   - **Network (GridConnect TCP)** (desktop app only): enter the hub's address as `host:port`, for example `jmri.local:12021`.
2. Press **Save settings**, then **Connect** in the top bar. Tick *Connect automatically when the app opens* if you want it to connect by itself next time.
3. Wait for **Online** in the top bar.

The panel then asks every device on the bus for the current state of each block and turnout, so it is up to date straight away rather than waiting for something to change. It asks again a few seconds later about anything that hasn't answered, and **⟳** in the diagram's top-right corner asks again whenever you like. If some blocks stay grey (*Unknown*), no device answered for their events; see [Troubleshooting](troubleshooting.md).

[Connecting to your layout](connecting.md) covers each connection type, sharing one adapter between programs, and Node IDs.

## Moving to another computer

Your layout and settings live in the browser (web app) or your settings folder (desktop app), so they don't follow you automatically.

- **Everything at once:** in **Settings**, press **Export settings…** to save one `.json` file with your settings and the layout. On the other computer, press **Import settings…** and choose that file. Both take effect at once.
- **Just the layout:** in the editor, press **Save to disk…** to save it as a `.json` file, and **Import…** on the other computer to load it. Import loads the file as a draft; press **Apply & save** to keep it.

Settings include the panel's Node ID, so if both computers will be on the bus at once, give one of them a new Node ID after importing.
