# Troubleshooting and FAQ

When something isn't working, press **Monitor** in the dock to open the bus monitor. Seeing what actually goes over the bus answers most questions quickly. Lines starting with → are sent by the panel; lines starting with ← are received, with the sender's alias.

## Connecting

### The app doesn't connect when it opens

A new install doesn't connect until you press **Connect**. Tick *Connect automatically when the app opens* in Settings to have it connect by itself. The simulator never connects by itself, even with that ticked.

### The status pill says *Error*

Hover over the pill (or tap it) to see the reason. The same message is shown at the bottom of Settings. Network connections retry every 3 seconds by themselves; serial connections need you to press **Connect** again.

### USB serial isn't offered, or fails, in the browser

Web Serial only works in **Chrome or Edge on a desktop or laptop computer**, with the page served over `https` or from `localhost`. Firefox, Safari and browsers on phones and tablets don't support it. Use one of those browsers, or the desktop app.

### "Click Connect to choose your USB LCC adapter"

The app tried to reconnect automatically but the browser hasn't been given permission for any port yet. Browsers only show the port picker after a click, so press **Connect** and choose the adapter.

### The serial port won't open ("Access denied", "Device or resource busy", "Permission denied")

- **Another program has it open.** A serial port can only be opened by one program at a time: close JMRI or the other panel, or [share the adapter through a hub](connecting.md#sharing-one-adapter-between-programs).
- **On Linux, you don't have permission.** Add yourself to the `dialout` group (`uucp` on Arch), then log out and back in.
- **The adapter was unplugged** and came back under a different name. Press **Refresh ports** in Settings and pick it again.

### The desktop app can't connect to JMRI

- In JMRI, make sure the LCC GridConnect hub is running on your LCC connection. It listens on port 12021 by default.
- Use the JMRI computer's name or IP address, not `localhost`, unless JMRI is on the same computer.
- Check the JMRI computer's firewall allows incoming connections on that port.

### Why can't the web app connect over the network?

Browsers don't let web pages open raw TCP connections, which GridConnect TCP needs. Use the desktop app for network connections, or plug a USB adapter into the computer running the browser.

### "Another node on the bus is using Node ID …"

Two nodes (two copies of System4Dispatch, or one and a device) have the same Node ID. Give this panel a different one in **Settings → This panel's OpenLCB node**. This most often happens when a browser profile or settings folder has been copied to another computer. See [Node IDs and aliases](connecting.md#node-ids-and-aliases).

## The panel

### All blocks show *Unknown* (grey)

Nothing answered for those events. The panel asks twice more in the first few seconds after connecting, for nodes that were still starting up. If a node powered up later than that, press **⟳** in the diagram's top-right corner to ask again. Otherwise, check that:

- the panel is **Online**;
- the block's event IDs match the detector's configuration exactly (compare them in JMRI's *LCC Configure Nodes*, or [learn them](editor.md#learning-event-ids-from-the-bus));
- the detector actually supports reporting its current state. Most LCC detectors do. If yours doesn't, the block shows its state after the first change.

### One block never goes back to clear

Check that the block's **Clear event** is filled in. With only an occupied event, the panel sees the block become occupied but never hears it clear.

### A turnout keeps blinking

The throw command went out, but no feedback came back. Check the feedback event IDs, check the point sensor, or, if the turnout has no sensors, delete both feedback events so the panel stops waiting for them.

### A turnout shows the wrong position

- **With feedback:** the panel shows what the sensor reports. Check the feedback events aren't swapped (normal and reverse).
- **Without feedback:** the panel shows the last command it saw, which may not match the points if someone threw them by hand with a control that doesn't send the same events. Press **N** or **R** to send the position you want.
- Check that the throw events aren't swapped in the Turnouts table.

### Tapping a turnout does nothing

- The panel must be **Online**.
- The turnout needs a **Throw normal** and **Throw reverse** event.
- The tile must have a turnout assigned (the editor's problems list says "turnout tile at … has no turnout assigned" if not).
- If *Ask before throwing a turnout* is on, check for a confirmation dialog.

### Throwing one turnout throws another too

They are [linked](editor.md#linking-turnouts). The Turnouts list shows **⇄** with the partner's ID. Set **Thrown with** to "— alone —" in the editor to unlink them.

## Throttles

### "no train found for address …"

No command station answered the search within 5 seconds. Make sure a command station is on the bus, is powered, and supports the OpenLCB **Traction** protocol. Some command stations only answer for locomotives that are in their roster.

### "train is controlled by another throttle"

Another throttle has already taken that locomotive. Release it on that throttle first, then press **Retry**.

### "train did not respond" or "train rejected command"

The command station found the train but didn't accept control, or doesn't support a command. Check the command station's firmware and documentation for OpenLCB Traction support.

### The throttle says "connection lost"

The connection dropped while the throttle was open. Once the panel is back **Online**, press **Retry** on the throttle.

### The locomotive stopped on its own

If the panel stops answering heartbeat requests (the computer went to sleep, the connection dropped), the command station stops the train for safety. Re-acquire it.

## The editor

### My changes disappeared

Editor changes are a draft until you press **Apply & save**. Reloading the page, closing the app or importing a settings file throws the draft away. Closing and reopening the Editor window keeps it.

### The arrow keys or Delete don't do anything

Keyboard shortcuts work when the grid has focus and the Editor window is in front. Click on the grid first.

### "Can't move there"

A move is refused if any selected tile would go off the grid or land on a tile that isn't selected. Make room first, or make the grid bigger.

### The block colours changed

Colours are assigned by position in the Blocks table, so deleting a block shifts the colours of those after it. They're only an editing aid; the panel always uses occupancy colours.

## Configuring nodes

### Configure is greyed out

That node doesn't offer a configuration description (CDI). Trains and some simple nodes don't have one.

### A node is missing from the Nodes list

The list fills in as nodes answer when the panel connects, and as they announce themselves. Press **⟳ Refresh** to ask again. A node that is powered off or on another bus segment won't appear.

### "did not acknowledge" or "did not reply in time"

The node didn't answer a configuration request within 3 seconds. Check it is still on the bus (it shows in the Nodes list), then press **⟳** beside the field or **Read all** to try again. See [Configuring LCC nodes](configuring-nodes.md#when-something-goes-wrong).

## General questions

### Where is my layout saved?

In the browser's local storage (web app) or in the `System4Dispatch` folder inside your system's configuration directory (desktop). See [Where things are saved](settings.md#where-things-are-saved). Use **Export settings…** in Settings to keep a copy of your settings and layout together, or **Save to disk…** in the editor for just the layout.

### Where did the tabs go?

The app now uses windows instead of tabs. Every view that used to be a tab has a button in the dock along the bottom. Throttles each get their own window: press **+ Throttle**.

### Can several people run panels at once?

Yes. Each running copy is its own node on the bus with its own Node ID. Panels see each other's turnout commands, and any number can show the same blocks. To share one USB adapter between several programs, use a [hub](connecting.md#sharing-one-adapter-between-programs).

### Does it work offline?

The web app keeps a copy of itself in the browser after the first visit, so it opens without an internet connection (handy at a club layout with no Wi-Fi). Chrome and Edge can also install it as an app. The desktop app never needs the internet.

### Does it work on a phone or tablet?

The panel, throttles and editor all work with touch screens. On a narrow screen the front window fills the screen and the dock switches between windows, and the editor has **⊕ Multi** for selecting several tiles without a Shift key. But mobile browsers don't support Web Serial, so a phone or tablet can only use the simulator. For real hardware, use a desktop or laptop computer.

### Does it support signals, routes, or block detection by transponder?

Not yet. System4Dispatch currently handles block occupancy, turnouts (with optional feedback and linking), turntables, throttles and node configuration.

### Where can I get the latest development build?

Nightly builds are published as a GitHub pre-release tagged `nightly`, with an AppImage and a Flatpak bundle, on the [releases page](https://github.com/UncommonModels/System4Dispatch/releases). They may be less stable than tagged releases.

### Where do I report a problem or ask for a feature?

On the project's [GitHub issues](https://github.com/UncommonModels/System4Dispatch/issues). A copy of the bus monitor output and your exported layout file help a lot.
