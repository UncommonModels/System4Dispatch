# Settings

The **Settings** tab is a form: change what you like, then press **Save settings** at the bottom. The button is greyed out until something has changed.

## Connection

- **Simulator**, **USB serial (GridConnect)**, and in the desktop app **Network (GridConnect TCP)**. See [Connecting to your layout](connecting.md).
- For USB serial on desktop: **Port**, **Baud rate** and **Refresh ports**. In the web app: **Baud rate** only, because the browser chooses the port.
- For network: **Hub address (host:port)**, default `localhost:12021`.
- **Connect automatically when the app opens** (on by default).

If you change the connection while connected, saving disconnects and reconnects with the new settings. The exception is USB serial in the web app: the browser only shows its port picker after a click, so press **Connect** yourself.

If the connection has failed, the reason is shown next to the Save button.

## This panel's OpenLCB node

- **Node ID**: this copy of System4Dispatch's unique ID on the bus. A random one is generated the first time the app runs. It must be six hex bytes, for example `05.01.01.01.8D.42`; anything else is refused with an explanation. Changing it reconnects the panel under the new ID.
- Below that, the current Node ID and, when Online, the alias.
- A warning appears if another node on the bus is using the same Node ID.

See [Node IDs and aliases](connecting.md#node-ids-and-aliases).

## Appearance

**Match system**, **Light** or **Dark**. *Match system* follows your operating system's setting. On desktop it also sets the colour of the window frame.

## Operation

- **Ask before throwing a turnout**: show a confirmation before each throw. Useful for public operating sessions and touch screens.
- **Show bus monitor on the panel**: show decoded bus traffic on the Panel tab (and at the bottom of Settings). See [The bus monitor](panel.md#the-bus-monitor).

## About

The version number, and whether you're running the desktop app or the web app.

## Where things are saved

| | Web app | Desktop app |
|---|---|---|
| Settings | browser local storage, key `s4d.settings` | `s4d_settings.json` in the config folder |
| Applied layout | browser local storage, key `s4d.layout` | `s4d_layout.json` in the config folder |

The desktop config folder is `System4Dispatch` inside your system's configuration directory:

| System | Folder |
|---|---|
| Linux | `~/.config/System4Dispatch/` (or `$XDG_CONFIG_HOME/System4Dispatch/`) |
| macOS | `~/Library/Application Support/System4Dispatch/` |
| Windows | `%APPDATA%\System4Dispatch\` |

Web app data belongs to the browser and the site: a different browser, a private window, or clearing site data starts afresh with the demo layout and a new Node ID. Use **Export…** in the editor to keep a copy of your layout.

The web app also works **offline** once it has been opened while online: it keeps a copy of itself in the browser. In Chrome and Edge you can install it as an app from the address bar.
