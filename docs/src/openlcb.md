# How it works: OpenLCB

This chapter explains what System4Dispatch actually says on the LCC bus, from the simple idea of events down to individual CAN frames. You don't need any of it to run the panel, but it helps when a block won't light up or a throttle won't acquire, and it's what you'll see in the [bus monitor](panel.md#the-bus-monitor).

The protocol work is done by the [rolcb](https://github.com/UncommonModels/rolcb) library. Its [documentation](https://www.uncommonmodels.com/rolcb/) has fuller guides (events, throttles, the hub) and complete reference tables of message types and frame layouts. The authoritative source is the set of OpenLCB Standards published at <https://openlcb.org>.

## Nodes

Everything on an LCC bus is a **node**: a detector board, a turnout controller, a command station, JMRI, and each running copy of System4Dispatch. Every node has a unique 48-bit **Node ID**, such as `05.01.01.01.8D.42`.

System4Dispatch is one node. It identifies itself (in OpenLCB's *Simple Node Information*) with manufacturer "Uncommon Models", model "System4Dispatch" and the app version, so it shows up by name in tools like JMRI's *LCC Configure Nodes*. When asked which protocols it supports (*Protocol Support Inquiry*), it answers Event Exchange, Simple Node Information and Datagram.

## Events, producers and consumers

Most of LCC is built on **events**. An event is just a 64-bit number, written as eight hex bytes like `05.01.01.01.22.00.00.10`. It carries no meaning of its own: it means "the thing we agreed on just happened". Each device is configured with the events it cares about:

- A node that *notices* something **produces** an event. A detector produces "block 1 occupied" when a train arrives.
- A node that *does* something when an event happens **consumes** it. A turnout driver consumes "throw turnout 1 reverse".

Event IDs are usually taken from the producing node's own Node ID range, which guarantees they are unique: a detector with Node ID `05.01.01.01.22.00` typically offers events starting `05.01.01.01.22.00.…`.

For System4Dispatch:

| Thing in the layout file | Events | The panel… |
|---|---|---|
| Block | `occupied_event`, `clear_event` | **consumes** them (listens) |
| Turnout feedback | `feedback.normal_event`, `feedback.reverse_event` | **consumes** them |
| Turnout throw | `normal_event`, `reverse_event` | **produces** them (sends them when you tap) |

### Event reports (PCER)

When an event happens, its producer broadcasts a **Producer/Consumer Event Report**, usually shortened to **PCER**. That's the message you see most in the bus monitor, as `Pcer(EventId(…))`.

- A PCER of a block's `occupied_event` or `clear_event` sets the block to *Occupied* or *Clear*.
- A PCER of a feedback event sets the turnout's position and ends any pending throw.
- Throwing a turnout sends a PCER of its `normal_event` or `reverse_event`. The panel also listens for PCERs of these events from *other* nodes (another panel, JMRI, a fascia button configured to send the same event). For a turnout without feedback, that becomes the shown position. For a turnout with feedback, it marks a throw as pending until the feedback arrives.

The panel also keeps the last eight distinct events seen in PCERs, newest first. That list is what the editor's [Recent bus events](editor.md#learning-event-ids-from-the-bus) shows.

### State queries at startup

A PCER only says that something *changed*. If the panel waited for changes, every block would stay *Unknown* until a train moved. Instead, OpenLCB lets any node ask about an event's **current state**:

- **Identify Producer** *event*: "who produces this event, and is its condition currently true?" Each producer answers **Producer Identified** with a state: *Valid* (true now), *Invalid* (false now) or *Unknown*.
- **Identify Consumer** *event*: the same question for consumers, answered with **Consumer Identified**.

As soon as the panel is logged in (and again whenever a new layout is applied, or you press **⟳** on the panel), it sends:

1. **Identify Producer** for every block occupied and clear event, every turnout feedback event, every turnout throw event, and every turntable track's *arrived* event. A detector answers *Valid* for `occupied_event` if the block is occupied right now, and *Invalid* if not, and the opposite for `clear_event`. Point sensors answer the same way for their feedback events. Other panels and JMRI answer for the throw events they produce, saying which position they last sent.
2. **Identify Consumer** for every turnout throw event. Many turnout drivers report the last command they acted on as *Consumer Identified Valid*, which gives the panel a starting position even without sensors.

For a turnout **without** feedback, a *Valid* answer on a throw event sets its position. For a turnout **with** feedback, it only sets the position while it is still unknown; when the feedback sensors answer, or report a change, they take over.

Nodes that are still starting up (after a power-on, say) miss the first round. So 3 seconds later the panel asks again about every block, turnout and turntable that is still unknown, and once more 3 seconds after that.

The panel also answers these questions itself. When it logs in, it announces itself as a consumer of its block and feedback events and a producer of its throw events. After each throw it updates its own producer state (the event it just sent becomes *Valid* and the opposite one *Invalid*), so other panels that ask get the right answer.

```text
  System4Dispatch                     Detector node
       │  Identify Producer 05.01.01.01.22.00.00.10 │
       │ ─────────────────────────────────────────► │
       │  Producer Identified Valid  …00.00.10      │   block 1 is occupied now
       │ ◄───────────────────────────────────────── │
       │  Identify Producer 05.01.01.01.22.00.00.11 │
       │ ─────────────────────────────────────────► │
       │  Producer Identified Invalid …00.00.11     │   (and so not clear)
       │ ◄───────────────────────────────────────── │
       │                 … later …                  │
       │  PCER 05.01.01.01.22.00.00.11              │   block 1 just cleared
       │ ◄───────────────────────────────────────── │
```

## Node configuration

The [Nodes and Configure windows](configuring-nodes.md) use three more parts of OpenLCB.

**Finding nodes.** When it logs in, the panel sends a global **Verify Node ID**. Every node answers **Verified Node ID**, and the panel asks each for its **Simple Node Information** (manufacturer, model, versions, user name) and its **Protocol Support**. Nodes that log in later announce themselves with Initialization Complete, and the panel asks them the same. A node that says it supports the *CDI* protocol can be configured. **⟳ Refresh** sends Verify Node ID again.

**Datagrams.** Configuration requests travel as *datagrams*: addressed messages of up to 72 bytes, split over several CAN frames. The receiver answers every datagram with **Datagram Received OK** or **Datagram Rejected**. The panel acknowledges the read and write replies it is waiting for and rejects any other datagram sent to it: it has no configuration memory of its own, so a tool that tries to configure the panel gets a clear "not supported" instead of waiting. If a node rejects one of the panel's datagrams with a "try again" code (it was busy), the panel sends it again, up to 3 times.

**Memory Configuration.** A node's settings live in numbered *memory spaces*. The panel uses three:

| Space | Holds |
|---|---|
| `FF` | The CDI: an XML document describing the other spaces. |
| `FD` | The node's configuration. |
| `FB` | User info (name and description), for nodes whose CDI has `<acdi/>`. |

Each request is a datagram to the node: *read* (space, address, how many bytes, at most 64) or *write* (space, address, data). The node acknowledges it, then sends a reply datagram with the data or an error code. The panel waits 3 seconds for the acknowledgement. For the reply, it waits as long as the node said it would need (the *reply pending* flag in its Datagram Received OK), or 3 seconds if it didn't say.

The panel keeps **one request in flight** at a time and queues the rest, so a slow node is never flooded. The CDI is read 64 bytes at a time from space `FF` until the end of the document. After every write, the panel sends **Update Complete**, which tells the node to act on its new settings. **Restart node** sends **Reset/Reboot**.

```text
  System4Dispatch                         Node
       │  Datagram: read FD @ 0, 64 bytes   │
       │ ─────────────────────────────────► │
       │  Datagram Received OK (reply pending)
       │ ◄───────────────────────────────── │
       │  Datagram: read reply FD @ 0, data │
       │ ◄───────────────────────────────── │
       │  Datagram Received OK              │
       │ ─────────────────────────────────► │
```

## CAN aliases and logging in

An LCC bus is a CAN bus, and a CAN frame's identifier has room for only 29 bits: not enough for a 48-bit Node ID plus a message type. So each node picks a short 12-bit **alias** (three hex digits, like `285`) when it joins the bus and puts that in every frame instead. The alias is not configured anywhere; the node chooses it, checks nobody else has it, and defends it.

When System4Dispatch connects, it logs in like this:

1. **CID** (*Check ID*): four frames, CID7 to CID4, each carrying the candidate alias and 12 bits of the Node ID. They say "I'd like alias 285; does anyone object?"
2. It waits **200 ms**. If any frame from alias 285 appears in that time, someone already has it, and the node picks another candidate and starts over.
3. **RID** (*Reserve ID*): "alias 285 is mine."
4. **AMD** (*Alias Map Definition*), carrying the full Node ID: "alias 285 belongs to Node ID 05.01.01.01.8D.42." Other nodes record the mapping.
5. **Initialization Complete**, then **Producer Identified** and **Consumer Identified** for every event the panel uses.

This is the brief *Logging in…* state in the top bar. After it, the panel is **permitted** to send messages: the pill shows **Online · 285**, and the [startup state queries](#state-queries-at-startup) go out.

Once logged in, the node keeps defending its alias. If another node sends CID frames for the same alias, the panel answers with RID ("taken"). If another node actually *uses* the alias, the panel gives it up with **AMR** (*Alias Map Reset*) and logs in again with a new one.

If the panel sees another node announce the **same Node ID** as its own, it records a duplicate, and Settings shows a warning. Two nodes with the same Node ID break the rules of the protocol, so give one of them a new ID.

When the connection drops, the panel forgets its alias and all block and turnout states. When the connection returns, it logs in again from step 1.

## The Traction protocol

Throttles use OpenLCB's **Traction Control** protocol. In OpenLCB every locomotive is a **train node**. For DCC locomotives, the train node is virtual: it lives inside the command station, which turns traction commands into DCC packets on the rails.

### Finding the train

The panel doesn't know the train node's alias, so it **searches** for it with a special event. The *Train Search* protocol encodes the DCC address and some flags into an event ID in the range `09.00.99.FF.xx.xx.xx.xx`:

- the address as decimal digits, one per hex nibble, padded with `F`, and
- flag bits: *allocate* (create the train if it doesn't exist), *exact match*, *address only*, *DCC*, and *long address* (always set for addresses above 127, or when you tick **Long address**).

For short address 3 the search event is `09.00.99.FF.FF.FF.F3.E8`; for long address 1234 it is `09.00.99.FF.FF.12.34.EC`.

The panel sends **Identify Producer** for that event. The command station creates the train node if necessary (because of the *allocate* flag), and the train node answers **Producer Identified**. The source alias of that answer is the train.

### A throttle session

```text
  System4Dispatch (alias 285)                Train node for DCC 3 (alias 6A1)
       │                                              │
       │  Identify Producer 09.00.99.FF.FF.FF.F3.E8   │   "Searching…" (5 s timeout)
       │ ───────────────────────────────────────────► │
       │  Producer Identified Valid (same event)      │
       │ ◄─────────────────────────────────────────── │
       │                                              │
       │  Traction: Assign Controller (our Node ID)   │   "Taking control…" (3 s timeout)
       │ ───────────────────────────────────────────► │
       │  Traction reply: Assign Controller, result 0 │   "In control"
       │ ◄─────────────────────────────────────────── │
       │  Query Speed, Query Function F0 … F4         │   sync the card with the train
       │ ───────────────────────────────────────────► │
       │  replies: current speed, function states     │
       │ ◄─────────────────────────────────────────── │
       │                                              │
       │  Set Speed / Set Function / Emergency Stop   │   you drive
       │ ───────────────────────────────────────────► │
       │                                              │
       │  Heartbeat request (timeout N s)             │   train: "still there?"
       │ ◄─────────────────────────────────────────── │
       │  Noop                                        │   panel answers at once
       │ ───────────────────────────────────────────► │
       │                                              │
       │  Release Controller (our Node ID)            │   ✕ pressed
       │ ───────────────────────────────────────────► │
```

In words:

1. **Search.** Identify Producer for the search event. If no train answers within 5 seconds, the throttle fails with "no train found for address …".
2. **Assign Controller.** An addressed *Traction Control Command* carrying the panel's Node ID. The train replies with a result: 0 means accepted. Anything else means it already has a controller ("train is controlled by another throttle"). No reply within 3 seconds: "train did not respond".
3. **Sync.** The panel asks for the current speed and the state of F0 to F4, and updates the card from the replies.
4. **Drive.** *Set Speed* carries the speed as a 16-bit floating-point number in metres per second, with the sign bit as direction. The panel sends slider step *n* as *n* mph. *Set Function* carries the function number and on/off. *Emergency Stop* stops immediately. Commands are only sent while the throttle is *In control*.
5. **Heartbeat.** The train may send a *heartbeat request* asking its controller to send something within a timeout. The panel replies with a *Noop* (a do-nothing management command). If no reply comes in time, the train stops itself, which protects against a throttle that vanished.
6. **Release.** *Release Controller* with the panel's Node ID, after which another throttle can take the train.

If the train answers a command with *Optional Interaction Rejected* (it doesn't support that command), the throttle fails with "train rejected command (error …)".

The byte layout of each command and reply is in the rolcb [throttle guide and reference tables](https://www.uncommonmodels.com/rolcb/).

## GridConnect framing

Between the computer and the bus, every CAN frame travels as one line of **GridConnect** text:

```text
  :X19490ABCN0102030405060708;
  │└──┬───┘│└──────┬───────┘└ end of frame
  │   │    │       └ 0 to 8 data bytes, as hex pairs
  │   │    └ N = normal frame
  │   └ 29-bit CAN identifier, in hex
  └ X = extended (29-bit) frame
```

For an OpenLCB message, the identifier is made of:

- `19` at the top: the fixed "OpenLCB message" bits plus the frame type,
- three hex digits of **message type** (the *CAN-MTI*), and
- three hex digits of **source alias**.

So in `:X195B4285N0501010122000100;`, `5B4` is the message type (PCER), `285` is the sender's alias, and the data `05 01 01 01 22 00 01 00` is the event ID `05.01.01.01.22.00.01.00`.

Some message types you'll meet:

| CAN-MTI | Message |
|---|---|
| `100` | Initialization Complete |
| `5B4` | PCER (event report) |
| `914` | Identify Producer |
| `544` / `545` / `547` | Producer Identified Valid / Invalid / Unknown |
| `8F4` | Identify Consumer |
| `4C4` / `4C5` / `4C7` | Consumer Identified Valid / Invalid / Unknown |
| `490` | Verify Node ID (global) |
| `170` | Verified Node ID |
| `A28` / `A48` | Datagram Received OK / Datagram Rejected |
| `5EB` | Traction Control Command |
| `1E9` | Traction Control Reply |
| `068` | Optional Interaction Rejected |

Login frames (CID, RID, AMD, AMR) are *control frames* with a different identifier layout; for example the AMD for alias `285` and Node ID `05.01.01.01.22.00` is `:X10701285N050101012200;`. *Addressed* messages, such as traction commands, start their data with the destination alias, and longer ones are split over several frames. Datagrams use their own frame types instead (identifiers starting `1A` to `1D`), with the destination alias in the identifier.

System4Dispatch writes frames one per line. When reading, it accepts any mix of line endings, and frames split across reads. The same text format is used over USB serial and over TCP, which is why one implementation covers USB adapters, JMRI, Ethernet buffers and `rolcb-hub` alike.

The full identifier layout, every message type and the addressed-message framing rules are tabulated in the rolcb [protocol reference](https://www.uncommonmodels.com/rolcb/).
