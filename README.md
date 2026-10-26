# System4Dispatch

A **dispatch panel and throttle** for model railroads running **OpenLCB / LCC**. It runs in the browser or as a desktop app.

- Draws your CTC-style track diagram and shows **live block occupancy**.
- **Throws turnouts** with a tap, and shows feedback from the points when you have it.
- **Throttles**: acquire any DCC address through an LCC command station (Traction protocol). You get speed, direction, F0–F28 and emergency stop.
- **Layout editor**: paint track, assign blocks and turnouts, and enter event IDs. Every block gets its own colour and name tag. You can select many tiles at once (drag a box or shift-click) and link turnouts such as crossover ends so they always throw together. You can also *learn* an event ID by pressing a button on the layout.
- **Node configuration**: list the LCC nodes on the bus and read and change their settings from their CDI, like JMRI's *Configure Nodes*.
- A **windowed desktop**: the panel, any number of throttles, node configuration, the editor and the bus monitor each open in their own window, from a dock along the bottom.
- A **built-in simulator**, so you can try everything without hardware.
- Written in Rust with [Dioxus](https://dioxuslabs.com): the same app compiles to WebAssembly for the browser and to a native desktop app. The OpenLCB stack is [`rolcb`](https://github.com/UncommonModels/rolcb).

**Documentation:** https://www.uncommonmodels.com/System4Dispatch/docs/ (sources in [`docs/`](docs/), built with mdBook)

## Ways to connect

All connections speak **GridConnect**, the text form of LCC's CAN frames.

| Connection | Web app | Desktop app |
|---|---|---|
| **USB adapter** (RR-CirKits LCC Buffer-USB, TCS, SPROG CANISB…) | ✓ (default) via Web Serial (Chrome or Edge on a desktop computer) | ✓ pick the port in Settings |
| **GridConnect TCP**: JMRI's LCC hub, an Ethernet LCC buffer, `rolcb-hub` (port 12021) | — (browsers can't open TCP sockets) | ✓ (default) |
| **Simulator** | ✓ | ✓ |

The app doesn't connect until you press **Connect**, unless you tick *Connect automatically when the app opens* in Settings. The simulator always waits for a click. To try the demo, choose **Simulator** in Settings, press **Save settings**, then **Connect**.

- **Web app:** https://www.uncommonmodels.com/System4Dispatch/. You can also install it as a PWA so it works offline.
- **Desktop app:** download from the [releases page](https://github.com/UncommonModels/System4Dispatch/releases):

  | Platform | Download |
  |---|---|
  | Linux x86_64 | `.deb`, `.rpm`, `.AppImage` or `.flatpak` |
  | Linux arm64 (Raspberry Pi 4/5, 64-bit OS) | `.deb` or `.rpm` |
  | macOS | `.dmg` (Apple silicon or Intel) |
  | Windows | `.msi` or `-setup.exe` |
  | Self-hosted web app | `-web.zip` |

  Per-platform instructions are in the [Installing](https://www.uncommonmodels.com/System4Dispatch/docs/install.html) chapter of the docs. Daily testing builds (5am US Eastern) are on the [`nightly` pre-release](https://github.com/UncommonModels/System4Dispatch/releases/tag/nightly).

Each running copy is its own OpenLCB node with a unique Node ID, generated on first run and editable in Settings. Several dispatchers and throttles can run at once.

A serial port can only be opened by one program. To share one USB adapter between JMRI and several panels, run [`rolcb-hub`](https://github.com/UncommonModels/rolcb) and connect everything to it over TCP:

```sh
rolcb-hub --serial /dev/ttyACM0          # GridConnect TCP on :12021
```

## Layout files

A layout is a JSON file: a grid of track tiles, plus the blocks and turnouts they belong to and the events that drive them. See [`crates/core/demo-layout.json`](crates/core/demo-layout.json).

```json
{
  "version": 1,
  "name": "Hilltown Subdivision",
  "grid": { "cols": 32, "rows": 12 },
  "blocks": [
    { "id": "B1", "name": "West Main",
      "occupied_event": "05.01.01.01.22.00.00.10", "clear_event": "05.01.01.01.22.00.00.11" }
  ],
  "turnouts": [
    { "id": "T1", "name": "Hilltown West",
      "normal_event": "05.01.01.01.22.00.01.00", "reverse_event": "05.01.01.01.22.00.01.01",
      "feedback": { "normal_event": "05.01.01.01.22.00.02.00", "reverse_event": "05.01.01.01.22.00.02.01" },
      "linked": "T2" }
  ],
  "elements": [
    { "type": "track", "x": 2, "y": 6, "shape": "straight", "rot": 0, "block": "B1" },
    { "type": "track", "x": 6, "y": 6, "shape": "turnout_left", "rot": 0, "block": "B2", "turnout": "T1" },
    { "type": "label", "x": 12, "y": 3, "text": "Hilltown" }
  ]
}
```

- **Shapes:** `straight`, `curve`, `bumper`, `crossing`, `crossing_diag`, `turnout_left`, `turnout_right`, `wye`. `rot` turns the tile clockwise in 45° steps (0–7).
- **Blocks:** the panel sends *Identify Producer* for each event when it connects (and asks again 3 s and 6 s later about anything still unknown), so it learns the current state straight away. *Producer Identified Valid/Invalid* and *PCER*s keep it up to date.
- **Turnouts:** tapping sends the throw event as a PCER. With feedback events, the turnout blinks until the points report back. Without feedback, it shows the last command seen on the bus. `linked` names a second turnout that is always thrown with this one (both sides name each other); `"linked_inverted": true` sends it to the opposite position.

Layouts are saved in the browser (web) or your config folder (desktop), and can be exported and imported as files. **Export settings…** in Settings writes one file with your settings and the layout together.

## Building

You need Rust, the `wasm32-unknown-unknown` target and the Dioxus CLI. [rolcb](https://github.com/UncommonModels/rolcb) must be checked out next to this repo (`../rolcb`).

```sh
rustup target add wasm32-unknown-unknown
cargo install dioxus-cli --version 0.7.10 --locked

cargo test                                                    # core logic tests
dx serve -p system4dispatch                                   # web app at http://localhost:8080
dx serve -p system4dispatch --platform desktop                # desktop app
dx bundle -p system4dispatch --platform web --release         # static site
dx bundle -p system4dispatch --platform desktop --release     # installers
```

On Linux the desktop build needs WebKitGTK and friends. On Fedora: `sudo dnf install webkit2gtk4.1-devel gtk3-devel libxdo-devel libsoup3-devel systemd-devel`. On Debian/Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libudev-dev`.

Repository layout:

```
crates/core   layout model, Dispatcher (OpenLCB state machine), CDI parser and node configuration client, Simulator; host-testable
crates/app    the Dioxus app; src/desktop.rs is the window manager and dock, src/platform/{web,desktop}.rs hold the per-platform links and storage
```

GitHub Actions run the tests, publish the web app and the docs to GitHub Pages, attach installers for every platform (plus a Flatpak and a self-hostable web app) to tagged releases, and publish a nightly testing pre-release (tag `nightly`) for every platform at 5am US Eastern. Build the Flatpak locally with the commands at the top of [`packaging/flatpak/com.uncommonmodels.System4Dispatch.yml`](packaging/flatpak/com.uncommonmodels.System4Dispatch.yml).

## License

MIT
