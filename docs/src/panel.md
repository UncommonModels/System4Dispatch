# The dispatch panel

The **Panel** window is the dispatcher's view of the railroad: the track diagram on the left (or on top, in a narrow window), and lists of turnouts and blocks beside it. It is the window the app opens with. If you close it, press **Panel** in the dock to open it again.

If the layout has no track yet, the panel says so and points you to the Editor in the dock. If the panel isn't Online, a banner across the diagram says "Not connected. Turnouts can't be thrown and states may be stale."

## Zooming and scrolling

The diagram starts fitted to the height of the window, so every row of the layout is in view; a long layout scrolls sideways. Use **+** and **−** in its top-right corner, or **Ctrl+wheel** (a pinch on a trackpad), to zoom; **Fit** goes back to fitting the height. **⟳** asks the layout again for every block's occupancy and every turnout's position (see [How the panel knows](#how-the-panel-knows)). **Drag** the diagram (or use the scroll bars, or swipe on a touch screen) to move around. A drag never throws a turnout; only a click does.

## Track colours

Each piece of track is drawn according to the block it belongs to:

| Look | Meaning |
|---|---|
| Solid, high contrast (light grey on the dark theme, dark slate on the light theme) | **Clear**: the detector says the block is empty. |
| Red | **Occupied**: the detector says something is in the block. |
| Dim grey | **Unknown**: no detector has reported yet, or the connection dropped. |
| Mid grey | **No detection**: this track isn't part of any block. |

The block list next to the diagram shows the same thing as a coloured dot beside each block's name, plus the words *Clear*, *Occupied* or *Unknown*. Hover over a block's name to see its ID.

### How the panel knows

A block has up to two events: one its detector sends when the block becomes **occupied**, and one it sends when it becomes **clear**. The panel listens for both. It also asks the detectors for the current state as soon as it comes Online, so blocks normally show their real state within a moment of connecting, rather than staying grey until a train moves. Nodes that are still starting up can miss that first question, so the panel asks again about anything still unknown 3 seconds later, and once more 3 seconds after that. Press **⟳** in the diagram's top-right corner to ask about everything again at any time.

Enter both events whenever your detector has them. The startup query can work with just one: if a detector answers that the *occupied* event is currently **not** true, the panel shows the block as clear (and the other way round for the *clear* event). But live changes only arrive as event reports, so with only an *occupied* event configured, a block turns red when a train arrives and then stays red until the next time the panel connects.

## Turnouts

### Reading a turnout

A turnout tile has two routes through it: **normal** (usually straight through) and **reverse** (the diverging route).

- The route the points are **set for** is drawn in full, in the block's occupancy colour.
- The other route is drawn **thin and faded**.
- If the position is **unknown**, both routes are drawn thin.
- A **blinking** route means a command has been sent and the panel is waiting for the turnout to confirm it has moved.

In the Turnouts list, each turnout shows its name (hover for its ID), a badge with the position (*Normal*, *Reverse* or *?*), and **N** and **R** buttons. The badge is highlighted while a throw is pending. A turnout that is [linked](#linked-turnouts) to another shows a **⇄** badge naming its partner.

### Throwing a turnout

- **Tap the turnout on the diagram** to flip it to the other position. If its position is unknown, the first tap throws it to normal. Hover over a turnout tile to see its name and ID.
- **Press N or R** in the Turnouts list to send it to a specific position. Pressing the position it's already in sends the command again, which is a handy way to re-sync a turnout that someone moved by hand.

The buttons are disabled, and taps do nothing, while the panel isn't Online.

For public operating sessions or a busy club night, turn on **Ask before throwing a turnout** in Settings. The panel will then ask "Throw turnout Hilltown West?" before every throw.

### Feedback and pending throws

What happens after you throw depends on whether the turnout has **feedback events** configured (point-position sensors, or a turnout driver that reports its position).

**With feedback:**

1. You tap. The panel sends the throw command.
2. The route it is moving to blinks, and the badge is highlighted. This is the *pending* state.
3. When the feedback event arrives saying the points are in the new position, the blinking stops and the new route is drawn bright.

If a turnout keeps blinking, the command went out but no confirmation came back: the feedback event IDs might be wrong, the sensor might not be working, or the points might be stuck.

**Without feedback:**

The panel has no way to know where the points really are, so it shows the **last command seen on the bus**. That includes commands from this panel, from other panels and JMRI, and from fascia buttons that send the same events. When it first connects, the panel also asks the turnout drivers which position they last commanded, and asks other panels and JMRI which position they last sent, so it can show that instead of *?*.

A turnout **with** feedback also gets a starting position this way while its sensors haven't answered yet. As soon as a feedback event arrives, it takes over.

### Linked turnouts

Some turnouts must always move together. The classic case is a **crossover**: two turnouts back to back, where throwing only one end would derail the next train. In the editor you can **link** two turnouts. Then:

- Throwing either one, from the diagram or the list, throws both.
- Each shows a **⇄** badge naming the other in the Turnouts list.
- A link can be marked **opposite**, in which case the partner goes to the other position (normal when this one goes reverse, and the other way round). This suits pairs wired so that one's "normal" corresponds to the other's "reverse".

Each turnout can be linked to at most one other. See [Linking turnouts](editor.md#linking-turnouts) for how to set it up.

## Turntables

A turntable is drawn as its pit, roundhouse and tracks, with the bridge across the pit; the arrowhead marks the bridge's head end.

- **⟲** and **⟳** next to its name in the side list turn it one track counter-clockwise or clockwise, sending the turntable's step events, just like throwing a turnout.
- **Click a track** on the drawing (a wedge lights up as you hover) or press its button in the side list to turn there. The panel sends as many steps as it takes the short way round.
- **Bridge is at** tells the panel where the bridge is. The panel needs to know this before it can turn to a track, because it counts steps; if the tracks have *arrived* events, it learns the position by itself when it connects.

While the bridge is moving to a track with an *arrived* event, the side list shows **Turning…** and the target is drawn faintly, blinking. If it doesn't arrive within 90 seconds the turntable is marked **no feedback**. With *Confirm before throwing* on, the panel asks first.

## 3-way switches and slips

A 3-way switch or slip shows its set route bright and the others dim. Click it to step to the next route; the panel throws its two turnouts (the one going to normal first). Each turnout also has its own row in the Turnouts list. A crossing or slip whose two tracks are in different blocks shows each track's occupancy separately.

## Portals

An arrow with a name marks a track that leaves here and carries on at the other arrow with the same name, often on the opposite edge of the panel.

## The bus monitor

Press **Monitor** in the dock to open the bus monitor in a window of its own. The panel only logs bus messages while the Monitor window is open (or the setting below is on), so open it before you trigger whatever you want to watch. To keep the monitor in the Panel window instead, tick **Show bus monitor on the panel** in Settings: a *Bus monitor* box appears under the lists.

The monitor shows the last 80 messages, newest at the top, decoded into readable form:

```text
← 3F2 Pcer(EventId(05.01.01.01.22.00.00.12))
→ Pcer(EventId(05.01.01.01.22.00.01.01))
← 3F2 Pcer(EventId(05.01.01.01.22.00.02.01))
```

Lines starting with **→** were sent by this panel; lines starting with **←** were received, with the sender's alias first. It's useful for checking that a detector or button really is sending the event you think it is. [How it works](openlcb.md) explains the message names.

## STOP ALL

Whenever at least one throttle is open, a red **STOP ALL** button appears in the top bar, whichever window is in front. It sends an emergency stop to every train this panel is running. It does not affect trains run by other throttles.
