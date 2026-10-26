# Settings

Press **Settings** in the dock to open the Settings window. It is a form: change what you like, then press **Save settings** at the bottom. The button is greyed out until something has changed. Saved settings are loaded every time the app opens.

## Connection

- **Simulator**, **USB serial (GridConnect)**, and in the desktop app **Network (GridConnect TCP)**. A new install starts on Network in the desktop app and USB serial in the web app. See [Connecting to your layout](connecting.md).
- For USB serial on desktop: **Port**, **Baud rate** and **Refresh ports**. In the web app: **Baud rate** only, because the browser chooses the port.
- For network: **Hub address (host:port)**, default `localhost:12021`.
- **Connect automatically when the app opens** (off by default). It never starts the simulator, even when ticked: the simulator always waits for you to press **Connect**.

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
- **Show bus monitor on the panel**: show decoded bus traffic in the Panel window (and in the Settings window). The **Monitor** window in the dock shows the same thing without this setting. See [The bus monitor](panel.md#the-bus-monitor).

## Settings file

Settings are saved when you press **Save settings**. The layout is saved when you press **Apply & save** in the editor. This card shows where they are kept: the file names in the desktop app, or the browser's local storage in the web app (see the table below).

- **Export settings…** writes one `.json` file with all your settings and the current track layout (under the key `layout`). A download in the web app, a "Save as" dialog on desktop.
- **Import settings…** reads such a file and applies both at once. The settings are saved and take effect straight away (reconnecting if the connection changed). The layout replaces the current one, and any unapplied changes in the editor are thrown away.

Use it to back up your setup or move it to another computer. The file includes the panel's Node ID, so if both computers will be on the bus at the same time, give one of them a new Node ID afterwards.

Live state, such as block occupancy, turnout positions and open throttles, is never saved.

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

Web app data belongs to the browser and the site: a different browser, a private window, or clearing site data starts afresh with the demo layout and a new Node ID. Use **Export settings…** to keep a copy of your settings and layout, or **Save to disk…** in the editor for just the layout.

The web app also works **offline** once it has been opened while online: it keeps a copy of itself in the browser. In Chrome and Edge you can install it as an app from the address bar.
