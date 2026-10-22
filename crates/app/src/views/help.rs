//! Help / explainer page.

use dioxus::prelude::*;

#[component]
pub fn HelpView() -> Element {
    rsx! {
        div { class: "help-view",
            nav { class: "card toc",
                h3 { "Contents" }
                a { class: "docs-link", href: "https://www.uncommonmodels.com/System4Dispatch/docs/", target: "_blank", "📖 Full documentation ↗" }
                a { href: "#start", "Getting started" }
                a { href: "#connect", "Connecting to your layout" }
                a { href: "#panel", "Using the panel" }
                a { href: "#throttle", "Running trains" }
                a { href: "#editor", "Drawing your layout" }
                a { href: "#events", "OpenLCB in two minutes" }
                a { href: "#trouble", "Troubleshooting" }
            }

            section { class: "card", id: "start",
                h2 { "Getting started" }
                p {
                    "System4Dispatch is a dispatcher's panel and throttle for model railroads that use "
                    b { "OpenLCB / LCC" }
                    ". Use it in your browser with nothing to install, or as a desktop app that can also connect over the network."
                }
                ol {
                    li { "It opens with the " b { "Simulator" } " and a demo layout, so you can try everything right away." }
                    li { "Tap a turnout on the " b { "Panel" } " to throw it. Watch blocks turn red as simulated trains move." }
                    li { "Open " b { "Throttles" } ", enter any DCC address and press " b { "Acquire" } "." }
                    li { "When you're ready, draw your own layout in the " b { "Editor" } " and connect to real hardware in " b { "Settings" } "." }
                }
                p { class: "muted",
                    "Settings and your layout are saved automatically (in the browser, or in your settings folder for the desktop app). Use " b { "Export" } " in the editor to keep a copy or move it to another computer."
                }
            }

            section { class: "card", id: "connect",
                h2 { "Connecting to your layout" }
                p { "Choose a connection in " b { "Settings" } ", then press " b { "Connect" } " in the top bar." }
                div { class: "help-grid",
                    div {
                        h4 { "USB adapter" }
                        p {
                            "Plug a GridConnect USB adapter (RR-CirKits LCC Buffer-USB, TCS USB, SPROG CANISB…) into this computer. "
                            "Choose " i { "USB serial" } ", then press Connect."
                        }
                        p { class: "muted small", "In a browser this needs Chrome or Edge on a desktop computer; the browser asks which port to use, and remembers it. The desktop app lists the ports in Settings." }
                    }
                    div {
                        h4 { "Network (desktop app)" }
                        p {
                            "The System4Dispatch desktop app can also connect over the network to a GridConnect TCP hub: JMRI's LCC hub, an Ethernet LCC buffer, or "
                            code { "rolcb-hub" } ". Choose " i { "Network (GridConnect TCP)" } " and enter " code { "host:12021" } "."
                        }
                        p { class: "muted small",
                            "Browsers can't open TCP connections, so this option only appears in the desktop app. To share one USB adapter between JMRI and several panels, run "
                            code { "rolcb-hub --serial /dev/ttyACM0" } " and point everything at it."
                        }
                    }
                    div {
                        h4 { "Simulator" }
                        p { "A pretend layout and command station built in: blocks change on their own, turnouts report back, and any locomotive address can be acquired. Great for learning and for designing panels." }
                    }
                }
                h4 { "The status pill" }
                ul { class: "legend",
                    li { span { class: "pill" , "Offline" } " not connected." }
                    li { span { class: "pill warn", "Logging in…" } " connected; claiming an address on the bus (about ¼ second)." }
                    li { span { class: "pill ok", "Online · 7AD" } " ready. The code is this panel's short bus alias." }
                    li { span { class: "pill err", "Error" } " hover or tap for the reason; network connections retry by themselves." }
                }
            }

            section { class: "card", id: "panel",
                h2 { "Using the panel" }
                div { class: "help-grid",
                    div {
                        h4 { "Track colours" }
                        svg { class: "legend-svg", "viewBox": "0 0 6 3.2",
                            path { class: "track occ-clear", "d": "M0.3 0.5 L2 0.5" }
                            text { class: "label", "x": "2.4", "y": "0.7", "Clear" }
                            path { class: "track occ-occupied", "d": "M0.3 1.3 L2 1.3" }
                            text { class: "label", "x": "2.4", "y": "1.5", "Occupied" }
                            path { class: "track occ-unknown", "d": "M0.3 2.1 L2 2.1" }
                            text { class: "label", "x": "2.4", "y": "2.3", "Unknown" }
                            path { class: "track occ-none", "d": "M0.3 2.9 L2 2.9" }
                            text { class: "label", "x": "2.4", "y": "3.1", "No detection" }
                        }
                    }
                    div {
                        h4 { "Turnouts" }
                        p { "Tap a turnout on the diagram to flip it, or use the " b { "N" } "/" b { "R" } " (normal / reverse) buttons in the list." }
                        p { "Turnouts marked " b { "⇄" } " are linked (for example the two ends of a crossover): throwing either one throws both." }
                        p { "The route the points are set for is drawn bright; the other leg is faded. A " b { "blinking" } " leg means the command was sent and we're waiting for the turnout to confirm it moved." }
                        p { class: "muted small", "Turnouts without feedback sensors show the last command seen on the bus, from any panel or fascia button." }
                    }
                }
                p { class: "muted small", "Tip: turn on " i { "Ask before throwing a turnout" } " in Settings for public operating sessions, and " i { "Show bus monitor" } " to watch the raw traffic." }
            }

            section { class: "card", id: "throttle",
                h2 { "Running trains" }
                ol {
                    li { "Enter the locomotive's DCC address. Tick " b { "Long address" } " if the locomotive uses a long (4-digit) address below 128; addresses above 127 are always long." }
                    li { "Press " b { "Acquire" } ". The panel asks the LCC command station for that locomotive and takes control of it." }
                    li { "Use the slider for speed (0–126 steps), " b { "REV/FWD" } " for direction, and " b { "F0–F28" } " for lights, sound and other functions." }
                    li { "Press " b { "✕" } " to release the locomotive so another throttle can take it." }
                }
                p { b { "EMERGENCY STOP" } " stops one locomotive immediately; " b { "STOP ALL" } " in the top bar stops every train this panel is running." }
                p { class: "muted small",
                    "Needs an OpenLCB command station that supports the Traction protocol (for example a TCS CS-105). "
                    "If a train is already controlled by another throttle, you'll be told; release it there first."
                }
            }

            section { class: "card", id: "editor",
                h2 { "Drawing your layout" }
                p {
                    "The editor works like a drawing program: tools on the left, the drawing in the middle, and a panel on the right with "
                    b { "Properties" } ", " b { "Blocks" } " and " b { "Turnouts" } ". The File, Edit and View menus are at the top, and the status bar underneath shows the cell under the pointer."
                }
                ol {
                    li { "In the " b { "Blocks" } " tab, press " b { "+ New block" } " for each detection section and give it a name. Each block gets its own colour." }
                    li { "With nothing selected, " b { "Properties" } " lets you choose which block new tiles go in. Pick a track piece from the tool rail (or press " kbd { "1" } "–" kbd { "8" } ") and click or drag on the grid. A faint preview shows what you'll place; " kbd { "R" } " rotates it." }
                    li { "Placing a turnout or wye creates a turnout for it. Open it in the " b { "Turnouts" } " tab to enter its events." }
                    li { "Press " b { "Apply" } " (" kbd { "Ctrl" } "+" kbd { "S" } ") to use the layout on the panel. Until then your changes are a draft; " b { "Discard" } " throws them away." }
                }
                h4 { "Selecting and editing" }
                ul {
                    li { "Use the " b { "Select" } " tool (" kbd { "V" } "): click a tile, drag a box around several, or Shift/Ctrl-click to add and remove tiles. On a touch screen, choose View → " i { "Tap adds to selection" } "." }
                    li { "With tiles selected, the Properties tab changes their block or rotation. Arrow keys move them, " kbd { "Delete" } " removes them, and " kbd { "Ctrl" } "+" kbd { "C" } "/" kbd { "X" } "/" kbd { "V" } "/" kbd { "D" } " copy, cut, paste and duplicate." }
                    li { "Everything can be undone with " kbd { "Ctrl" } "+" kbd { "Z" } " and redone with " kbd { "Ctrl" } "+" kbd { "Y" } ". Zoom with " kbd { "+" } "/" kbd { "−" } " or Ctrl+scroll." }
                }
                h4 { "Seeing your blocks" }
                p {
                    "Every block is drawn in its own colour with a name tag; track that isn't in any block is thin and grey. "
                    "In the Blocks tab, click a block's colour swatch to highlight just that block, or open it and choose " b { "Select tiles" } "."
                }
                h4 { "Turnouts that move together" }
                p {
                    "Open a turnout in the Turnouts tab and choose another under " b { "Thrown together with" } ", for example the other end of a crossover. "
                    "Tick " i { "Throw it to the opposite position" } " if needed. Linked turnouts are always thrown together, from the diagram or the list."
                }
                h4 { "Learning event IDs" }
                p {
                    "You rarely need to type an event ID. Connect to the layout, then press a fascia button, throw a turnout by hand, or roll a car into a block. "
                    "The event appears under " b { "Recent bus events" } ", and the " b { "⤓" } " button next to any event field fills in the newest one."
                }
                p { class: "muted small",
                    "Use File → Export and File → Open to move a layout between computers."
                }
            }

            section { class: "card", id: "events",
                h2 { "OpenLCB in two minutes" }
                p {
                    "LCC devices talk using " b { "events" } ": 8-byte numbers written like "
                    code { "05.01.01.01.22.00.00.10" } ". A device that notices something " i { "produces" } " an event, and any device configured for it " i { "consumes" } " it."
                }
                ul {
                    li { b { "Blocks:" } " an occupancy detector produces one event when the block becomes occupied and another when it clears. Enter both." }
                    li { b { "Turnouts:" } " a turnout driver consumes a " i { "throw normal" } " and a " i { "throw reverse" } " event. The panel produces them when you tap." }
                    li { b { "Feedback (optional):" } " point-position sensors produce events when the points actually arrive. With them, the panel shows the real position." }
                }
                p {
                    "When it connects, the panel asks every device for the current state of each event, so it is up to date immediately rather than waiting for something to change. "
                    "Find a device's event IDs in its configuration tool, such as JMRI's " i { "LCC Configure Nodes" } ", or learn them as described above."
                }
                p { class: "muted small",
                    "Every browser running System4Dispatch is its own node on the bus and needs a unique " b { "Node ID" } " (Settings). One is generated for you; "
                    "if your club has an assigned range, use an ID from it."
                }
            }

            section { class: "card", id: "trouble",
                h2 { "Troubleshooting" }
                dl {
                    dt { "All blocks show “Unknown”." }
                    dd { "No device answered for those events. Check the event IDs match the detector's configuration, and that the panel is Online." }
                    dt { "A turnout keeps blinking." }
                    dd { "The command went out but no feedback came back. Check the feedback event IDs, or clear them if the turnout has no sensors." }
                    dt { "“No train found for address …”" }
                    dd { "No command station answered. Make sure one is on the bus and supports OpenLCB Traction; some need the locomotive to be in their roster first." }
                    dt { "“Another node is using Node ID …”" }
                    dd { "Two browsers (or a browser and a device) share a Node ID. Give this panel a different one in Settings." }
                    dt { "USB serial isn't offered or fails in the browser." }
                    dd { "Use Chrome or Edge on a desktop computer (phones and Firefox don't support it), or use the desktop app." }
                    dt { "The desktop app can't connect to JMRI." }
                    dd { "In JMRI, start the LCC GridConnect hub on your LCC connection (port 12021 by default), and make sure your firewall allows it." }
                    dt { "Two programs need the same USB adapter." }
                    dd { "A serial port can only be opened by one program. Run rolcb-hub with the adapter, then connect JMRI and each panel to the hub over TCP." }
                }
            }

            p { class: "muted small center",
                "System4Dispatch {env!(\"CARGO_PKG_VERSION\")} · built on "
                a { href: "https://github.com/UncommonModels/rolcb", target: "_blank", "rolcb" }
                " · "
                a { href: "https://github.com/UncommonModels/System4Dispatch", target: "_blank", "source code" }
            }
        }
    }
}
