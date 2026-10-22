# Connecting to your layout

Every connection System4Dispatch offers carries **GridConnect**: a plain-text form of the CAN frames that make up an LCC bus, one frame per line, like `:X19490ABCN;`. Almost every LCC USB adapter and every LCC network hub speaks it, so the app doesn't need device-specific drivers.

Choose the connection under **Settings → Connection**, press **Save settings**, then press **Connect** in the top bar. If *Connect automatically when the app opens* is ticked (it is by default), the app reconnects by itself the next time you open it.

| Connection | Web app | Desktop app |
|---|---|---|
| **Simulator** | yes (default) | yes (default) |
| **USB serial (GridConnect)** | yes, via Web Serial in Chrome or Edge on a desktop computer | yes, native serial port |
| **Network (GridConnect TCP)** | not offered | yes |

## USB adapters

Supported adapters include the **RR-CirKits LCC Buffer-USB**, the **TCS USB** adapter and the **SPROG CANISB**: anything that shows up as a serial port and speaks GridConnect. The default speed is 115200 baud. Most USB adapters ignore the baud rate, but you can change it in Settings if yours needs something else.

### In the web app (Web Serial)

Browsers reach USB serial devices through the **Web Serial** API. Today that means **Chrome or Edge on a desktop or laptop computer** (Windows, macOS, Linux or ChromeOS). Firefox, Safari, and browsers on phones and tablets don't support it. The page must also be served over `https` (the published site is) or from `localhost`.

1. Plug the adapter in.
2. In **Settings**, choose **USB serial (GridConnect)** and press **Save settings**.
3. Press **Connect** in the top bar. The browser shows a list of serial devices; pick your adapter.

The browser remembers that you allowed this port. The next time the app opens with auto-connect on, it reopens the same port without asking. If it hasn't been allowed yet, the status pill shows *Error* with the message "Click Connect to choose your USB LCC adapter", because browsers only show the port picker after a click.

If the browser doesn't support Web Serial, Settings shows a warning: "This browser doesn't support USB serial. Use Chrome or Edge, or the desktop app."

### In the desktop app

1. Plug the adapter in.
2. In **Settings**, choose **USB serial (GridConnect)**. A **Port** list appears with the serial ports on this computer: names like `/dev/ttyACM0` or `/dev/ttyUSB0` on Linux, `/dev/cu.usbmodem…` on macOS, or `COM3` on Windows. Press **Refresh ports** if you plugged the adapter in after opening Settings.
3. Choose the port, press **Save settings**, then **Connect**.

If the port was saved earlier but isn't plugged in now, it is listed as "(not found)".

On Linux your user needs permission to open the port. On most distributions (Debian, Ubuntu, Raspberry Pi OS, Fedora) that means being in the `dialout` group; on Arch it is `uucp`. Log out and back in after adding yourself.

### Serial links and disconnects

A serial port can only be open in **one program at a time**. If JMRI or another panel already has the adapter open, connecting fails. See [Sharing one adapter](#sharing-one-adapter-between-programs) below.

If the adapter is unplugged, the connection drops and the panel shows *Error*. Serial connections do **not** reconnect by themselves; plug the adapter back in and press **Connect**.

## Network: GridConnect TCP (desktop app only)

A **GridConnect TCP hub** relays LCC traffic between programs over the network. The convention is TCP port **12021**. Browsers can't open raw TCP connections, so this option only appears in the desktop app. The web app's Settings shows a note pointing you to the desktop app instead.

In **Settings**, choose **Network (GridConnect TCP)** and enter the hub's address as `host:port`. The default is `localhost:12021`. Then save and connect.

Things you can connect to:

- **JMRI's LCC hub.** In JMRI, open your LCC connection and start its GridConnect hub (it listens on port 12021 by default). Then enter the JMRI computer's name or IP address, for example `jmri.local:12021` or `192.168.1.20:12021`. Make sure the JMRI computer's firewall allows incoming connections on that port.
- **An Ethernet LCC buffer** or other network-attached LCC interface that offers a GridConnect TCP server. Enter its address and port.
- **`rolcb-hub`**, a small standalone hub from the [rolcb](https://github.com/UncommonModels/rolcb) project (see the next section).

Network connections **reconnect by themselves**. If the hub goes away (JMRI restarted, Wi-Fi dropped), the panel shows *Error* and tries again every 3 seconds until it gets through or you press **Disconnect**.

## Sharing one adapter between programs

Because a serial port can only be open in one program, you can't plug a USB adapter into a computer and use it from JMRI *and* System4Dispatch at the same time. The answer is to put a hub in front of it.

[`rolcb-hub`](https://github.com/UncommonModels/rolcb) opens the adapter once and offers it to any number of programs over GridConnect TCP:

```sh
rolcb-hub --serial /dev/ttyACM0          # GridConnect TCP on :12021
```

Then point everything at the hub:

- **JMRI:** create an LCC connection of type *CAN via GridConnect network interface* to `host:12021`.
- **System4Dispatch (desktop):** Network, `host:12021`.
- **More panels:** the same. Each copy of System4Dispatch is its own node on the bus.

```text
               USB
  LCC bus ─── adapter ─── rolcb-hub ──┬── TCP ── JMRI
                       (Raspberry Pi) ├── TCP ── System4Dispatch (dispatcher's desk)
                                      └── TCP ── System4Dispatch (yard office)
```

A Raspberry Pi in the layout room running `rolcb-hub` as a service makes a tidy setup. The hub relays every frame to every other connection and never echoes a frame back to its sender, which is exactly how a real CAN bus behaves, so the programs connected to it can't tell they aren't wired directly to the bus.

Installation, all the command-line flags (several serial ports, joining another hub, verbose traffic logging) and a systemd unit are in the rolcb [hub guide](https://www.uncommonmodels.com/rolcb/).

JMRI's own LCC hub works the same way if JMRI is the program that owns the adapter: JMRI opens the USB port, and the System4Dispatch desktop app connects to JMRI's hub over TCP.

## Node IDs and aliases

Every device on an LCC bus has a unique, permanent 48-bit **Node ID**, written as six hex bytes like `05.01.01.01.8D.42`. **Each running copy of System4Dispatch is its own node**, so several dispatchers and throttles can be on the bus at once, and each needs its own Node ID.

- On first run the app generates one of the form `05.01.01.01.8D.xx`, with a random last byte, and saves it, so it stays the same from then on.
- You can see and change it in **Settings → This panel's OpenLCB node**. It must be six hex bytes; dots, dashes, colons and spaces between bytes are all accepted.
- Changing it disconnects and reconnects the panel under the new ID.
- If you, or your club, have your own assigned Node ID range, use an ID from it. Otherwise the generated one is fine for a home layout, but with only 256 possible random values, two panels can occasionally pick the same one.

If another node on the bus announces the same Node ID, Settings shows a warning: "Another node on the bus is using Node ID … Choose a different one." Give one of them a new ID.

When the panel connects, it also picks a short 12-bit **alias** (three hex digits, like `7AD`) that it uses in every CAN frame instead of the full Node ID. The status pill shows it: **Online · 7AD**. You never need to set the alias; it is chosen and defended automatically, and may be different each time you connect. [How it works](openlcb.md#can-aliases-and-logging-in) explains the details.

## The status pill

| Pill | Meaning |
|---|---|
| **Offline** | Not connected. |
| **Connecting…** | Opening the port or TCP connection. |
| **Logging in…** | Connected; claiming an alias on the bus. Takes about a quarter of a second. |
| **Online · 7AD** | Ready. The code is this panel's alias. |
| **Error** | Something went wrong. Hover over (or tap) the pill to see why. Network connections retry by themselves; serial connections need you to press Connect. |

When the connection drops, every block and turnout goes back to *Unknown*, open throttles show "connection lost" (press **Retry** on each once you are back online), and turnouts can't be thrown until the panel is Online again.
