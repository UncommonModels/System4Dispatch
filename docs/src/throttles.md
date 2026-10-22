# Throttles

The **Throttles** tab runs locomotives through an LCC command station, using the OpenLCB **Traction** protocol. You can open as many throttles as you like, and they keep running while you switch to other tabs.

## What you need

- An **LCC command station that supports OpenLCB Traction**, for example a TCS CS-105. It turns the throttle's commands into DCC packets on the track. (The built-in simulator includes one that accepts any address.)
- The panel must be **Online**. The **Acquire** button is disabled, with the note "Connect first.", until it is.

Some command stations only answer for locomotives in their roster. If acquiring fails with "no train found", check your command station's documentation.

## Acquiring a locomotive

1. Type the locomotive's **DCC address** (1 to 10239).
2. Tick **Long address** if the locomotive uses a long (four-digit-style) address below 128, for example long address 0042. Addresses of 128 and above are always long, whether or not the box is ticked.
3. Press **Acquire** (or Enter).

A throttle card appears, headed by the address (with a small **L** for long addresses) and a status:

| Status | What's happening |
|---|---|
| *Searching…* | The panel has asked the bus "who is DCC address 3?". The command station creates a train for that address if it doesn't have one yet, and answers. |
| *Taking control…* | The panel has found the train and asked to become its controller. |
| *In control* | You're driving. The card reads the locomotive's current speed, direction and F0 to F4 from the command station, so it starts out matching the train. |
| *no train found for address 3* | Nothing answered within 5 seconds. |
| *train did not respond* | The train was found but didn't answer the request for control within 3 seconds. |
| *train is controlled by another throttle* | Someone else has it. Release it on that throttle first. |
| *train rejected command (error …)* | The train node doesn't support a command the panel sent. |
| *connection lost* | The connection dropped while the throttle was open. |
| *not connected* | The panel wasn't Online when you pressed Acquire. |

When a throttle has failed, a **Retry** button appears on its card. Press it (once the panel is Online) to search again.

## Driving

- **Speed slider:** 0 to 126 steps, shown as a number above it ("42 / 126").
- **◀ REV** and **FWD ▶** set the direction, keeping the current speed. The active direction is highlighted.
- **STOP** sets the speed to 0, keeping the direction. The train slows according to its decoder's deceleration settings.
- **EMERGENCY STOP** stops this locomotive immediately (DCC emergency stop, ignoring deceleration) and moves the slider to 0.
- **F0 to F28** toggle the decoder functions: headlight (F0, marked 💡), bell, horn, sound and so on, depending on the decoder. A lit button means the function is on. Every function is a simple on/off toggle; for a momentary function like a horn, press once to turn it on and again to turn it off.

All controls are disabled unless the card says *In control*.

**STOP ALL** in the top bar sends an emergency stop to every locomotive this panel is controlling.

### About speed steps

The slider has 126 steps, matching the 128-speed-step mode of DCC (0 is stop, plus 126 running steps; the remaining value is emergency stop). Speeds in OpenLCB are sent as a real-world speed rather than a step number. The panel sends step *n* as *n* mph, so full throttle is 126 mph, which LCC command stations treat as DCC speed step 126, full speed. How the command station handles locomotives set to 14 or 28 speed steps is up to the command station.

## Releasing a locomotive

Press **✕** in the corner of the card. The panel tells the train it is no longer its controller and closes the card. The locomotive keeps doing whatever it was doing (it does **not** stop), so stop it first if that's what you want. Another throttle (a handheld, JMRI, another panel) can now acquire it.

## Keep-alive

While you're in control, a train may ask its controller to prove it is still there (a *heartbeat* request). The panel answers automatically. If the panel disappears without releasing the train (the computer sleeps, the network drops), the command station notices the missing answers and stops the train by itself after its timeout. This is a safety feature of the Traction protocol, not something you need to manage.

The detailed message flow is in [How it works: the Traction protocol](openlcb.md#the-traction-protocol).
