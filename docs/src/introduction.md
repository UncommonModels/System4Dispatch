# Introduction

**System4Dispatch** is a dispatcher's panel and throttle for model railroads that run **OpenLCB**, the open standard that the NMRA publishes as **LCC** (Layout Command Control). It draws your track as a CTC-style diagram, shows which blocks are occupied as it happens, throws turnouts with a tap, and runs locomotives through an LCC command station.

- **Web app:** <https://www.uncommonmodels.com/System4Dispatch/>. Nothing to install.
- **Desktop app:** installers for Linux (deb, rpm, AppImage, Flatpak; x86_64 and arm64, including Raspberry Pi), macOS and Windows on the [releases page](https://github.com/UncommonModels/System4Dispatch/releases). See [Installing](install.md).
- **Source code:** <https://github.com/UncommonModels/System4Dispatch> (MIT licence).

## What it does

- **Live track diagram.** Each piece of track belongs to a detection *block*. When a block's occupancy detector reports a train, that block turns red on the panel.
- **Turnouts.** Tap a turnout to throw it. If the turnout has point-position sensors, the panel blinks until the points confirm they have moved. Two turnouts can be *linked* (for example the two ends of a crossover) so they always move together.
- **Throttles.** Acquire any DCC address through an LCC command station. You get a 126-step speed slider, direction, F0 to F28, emergency stop and a STOP ALL button.
- **Layout editor.** Paint track onto a grid and assign it to blocks and turnouts. Enter the LCC event IDs, or *learn* them by pressing a button on the layout. Every block gets its own colour in the editor, so you can see at a glance which track belongs to which block.
- **Built-in simulator.** A pretend layout and command station, so you can try everything (and design your panel) without any hardware.

## Web or desktop?

The web app and the desktop app are the same program, built from the same source. They differ only in how they reach your layout and where they keep their files.

| | Web app | Desktop app |
|---|---|---|
| Install | none; open the page (it can also be installed as an offline web app) | download an installer |
| USB LCC adapter | yes, in **Chrome or Edge on a desktop computer** (Web Serial) | yes, pick the serial port in Settings |
| Network (GridConnect TCP): JMRI, Ethernet LCC buffers, `rolcb-hub` | no: browsers can't open raw TCP connections | yes |
| Simulator | yes | yes |
| Where your layout and settings are saved | the browser's local storage | JSON files in your settings folder |

If you only have a USB adapter plugged into the computer in front of you, the web app is all you need. If you want to connect over the network (to JMRI, to a Raspberry Pi in the layout room, or to share one adapter between several programs), use the desktop app.

## A quick tour

The app has five tabs along the bottom (or side) of the window, and a top bar.

**The top bar** shows the layout name, a status pill (*Offline*, *Connecting…*, *Logging in…*, *Online · 7AD*, or *Error*), and a **Connect** or **Disconnect** button. While any throttle is open, a red **STOP ALL** button appears there too.

**Panel** is the dispatcher's view. The track diagram fills most of the screen. Clear blocks are drawn in a solid, high-contrast colour, occupied blocks in red, and blocks whose state isn't known yet in grey. Each turnout shows the route it is set for in full colour and the other route faded. Beside the diagram are lists of turnouts (with **N** and **R** buttons for normal and reverse) and blocks (with their state). You can also show a bus monitor there.

**Throttles** is where you run trains. Type a DCC address, press **Acquire**, and a throttle card appears with a speed slider, direction buttons, emergency stop and function buttons. You can open as many throttles as you like.

**Editor** is where you draw the panel. A palette of track pieces sits above a grid. Below the grid are tables of blocks and turnouts where you enter names and event IDs. Changes stay as a draft until you press **Apply & save**.

**Settings** chooses the connection (simulator, USB or network), the panel's OpenLCB Node ID, the colour scheme, and a few operating options.

**Help** is a short built-in guide. This site is the long version.

## Who this guide is for

The first part ([Getting started](getting-started.md) to [Settings](settings.md)) is for operators and layout owners. You don't need to know anything about programming or the details of OpenLCB.

The [Reference](layout-format.md) part describes the layout file format and explains exactly what goes over the wire, for people who want to hand-edit layout files or debug a connection.

The [For contributors](architecture.md) part explains how the code is organised and how to build, test and release it.

System4Dispatch is built on [rolcb](https://github.com/UncommonModels/rolcb), a Rust OpenLCB library from the same authors. Its API documentation and protocol guides are at <https://www.uncommonmodels.com/rolcb/>.
