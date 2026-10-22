# System4Dispatch

A browser-based **dispatch panel and throttle** for model railroads running **OpenLCB / LCC**.

- Draws your CTC-style track diagram and shows **live block occupancy**.
- **Throws turnouts** with a tap, and shows feedback from the points when you have it.
- **Throttles**: acquire any DCC address through an LCC command station (Traction protocol). You get speed, direction, F0–F28 and emergency stop.
- **Layout editor** in the browser: paint track, assign blocks and turnouts, and enter event IDs. You can also *learn* an event ID by pressing a button on the layout.
- A **built-in simulator**, so you can try everything without hardware.
- Runs entirely in the browser (Rust → WebAssembly, built with [Dioxus](https://dioxuslabs.com)). The OpenLCB stack is [`rolcb`](https://github.com/UncommonModels/rolcb).

## Ways to connect

| Setup | What you need | How |
|---|---|---|
| **One computer + USB adapter** | Chrome or Edge, and a GridConnect USB adapter (RR-CirKits LCC Buffer-USB, TCS, SPROG CANISB…) | Open the hosted app, then Settings → *USB serial*, then **Connect** |
| **Phones/tablets on the layout Wi-Fi** | A PC or Raspberry Pi running `s4d-bridge` | `s4d-bridge --serial /dev/ttyACM0`, then open `http://<pi>:8080` on every device |
| **Existing JMRI / LCC hub** | JMRI's LCC hub, or a buffer on TCP 12021 | `s4d-bridge --upstream jmri.local:12021`, then open `http://<bridge>:8080` |
| **Just looking** | Any modern browser | Settings → *Simulator* (the default) |

Each browser is its own OpenLCB node with a unique Node ID, generated on first run and editable in Settings. Several dispatchers and throttles can run at once.

### s4d-bridge

`s4d-bridge` is a single binary. It serves the web app, gives every browser a WebSocket into an OpenLCB hub, and connects that hub to your hardware:

```sh
s4d-bridge --serial /dev/ttyACM0 --layout club-layout.json
#   web app + WebSocket   http://0.0.0.0:8080   (/ws)
#   GridConnect TCP       0.0.0.0:12021         (JMRI can connect here too)
#   shared layout         every panel loads/saves club-layout.json
```

| Option | Meaning |
|---|---|
| `--listen ADDR` | Web server address (default `0.0.0.0:8080`) |
| `--upstream HOST:PORT` | Connect to another GridConnect hub (repeatable) |
| `--serial PATH` / `--baud N` | Attach a USB GridConnect adapter (repeatable) |
| `--tcp ADDR` / `--no-tcp` | GridConnect TCP listener for JMRI etc. (default `0.0.0.0:12021`) |
| `--layout FILE` | Share one layout file between all panels (created from the demo if missing) |
| `--static-dir DIR` | Serve the app from a directory instead of the embedded copy |

> Browsers block `ws://` connections from `https://` pages except to `localhost`. That's why the bridge serves its own copy of the app over plain http. Use the hosted (https) app for USB serial, and the bridge's URL for network connections.

If you only need a hub without the web app, rolcb also ships `rolcb-hub` (GridConnect TCP :12021 + WebSocket :12022).

## Layout files

A layout is a JSON file: a grid of track tiles, plus the blocks and turnouts they belong to and the events that drive them. See [`examples/demo-layout.json`](examples/demo-layout.json).

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
      "feedback": { "normal_event": "05.01.01.01.22.00.02.00", "reverse_event": "05.01.01.01.22.00.02.01" } }
  ],
  "elements": [
    { "type": "track", "x": 2, "y": 6, "shape": "straight", "rot": 0, "block": "B1" },
    { "type": "track", "x": 6, "y": 6, "shape": "turnout_left", "rot": 0, "block": "B2", "turnout": "T1" },
    { "type": "label", "x": 12, "y": 3, "text": "Hilltown" }
  ]
}
```

- **Shapes:** `straight`, `curve`, `bumper`, `crossing`, `crossing_diag`, `turnout_left`, `turnout_right`, `wye`. `rot` turns the tile clockwise in 45° steps (0–7).
- **Blocks:** the panel sends *Identify Producer* for each event at startup, so it learns the current state straight away. *Producer Identified Valid/Invalid* and *PCER*s keep it up to date.
- **Turnouts:** tapping sends the throw event as a PCER. With feedback events, the turnout blinks until the points report back. Without feedback, it shows the last command seen on the bus.

Layouts are stored in the browser. They can be exported and imported as files, or shared through the bridge's `--layout` file.

## Building

You need Rust, the `wasm32-unknown-unknown` target and the Dioxus CLI. [rolcb](https://github.com/UncommonModels/rolcb) must be checked out next to this repo (`../rolcb`).

```sh
rustup target add wasm32-unknown-unknown
cargo install dioxus-cli --version 0.7.10 --locked

cargo test                                  # core logic + bridge tests
dx serve -p system4dispatch                 # develop the web app at http://localhost:8080
dx bundle --platform web --release -p system4dispatch
cargo build --release -p s4d-bridge         # embeds the bundled app
```

Repository layout:

```
crates/core     layout model, Dispatcher (OpenLCB state machine), Simulator; host-testable
crates/app      the Dioxus web app
crates/bridge   s4d-bridge (axum + rolcb::hub)
```

GitHub Actions run the tests, publish the app to GitHub Pages, and attach `s4d-bridge` binaries for Linux (x86_64, arm64 / Raspberry Pi), macOS and Windows to tagged releases.

## License

MIT
