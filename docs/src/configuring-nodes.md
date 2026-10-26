# Configuring LCC nodes

Most LCC boards are set up by changing values in their configuration memory: which events an input sends, how long a servo takes to move, what the board is called. Each board describes its own settings in an XML document called its **CDI** (*Configuration Description Information*). System4Dispatch reads that description and turns it into a form, the same way JMRI's *LCC Configure Nodes* does.

The panel must be **Online** to see or change anything.

## The Nodes window

Press **Nodes** in the dock. The window lists every node on the bus. The panel asks every node to identify itself when it logs in (*Verify Node ID*), and adds nodes as they announce themselves later.

Each row shows:

- the node's **name**: the user name you gave it (in its *User info*), or else its manufacturer and model;
- its **Node ID**, such as `05.01.01.01.22.00`;
- its **manufacturer · model · software version**, as the node reports them. While the panel is still asking, this says *asking…*.

Press **⟳ Refresh** to ask the bus again, for example after powering up a new board.

**Configure** opens a window for that node. It is greyed out for nodes that don't offer a CDI, such as trains.

## The Configure window

Each node gets its own Configure window, with its own button in the dock. Pressing **Configure** for a node that already has a window brings that window to the front.

When the window opens, it downloads the node's CDI. The CDI is read from the node's memory space `0xFF`, 64 bytes at a time, so a big board can take a few seconds. Meanwhile the window shows "Reading the configuration description… *n* bytes".

The form then shows:

- one **card** per *segment* of the node's memory, with its name and description;
- **groups** that open and close when you click their heading. A group's values are read from the node when you open it, so a large board stays quick. **Replicated** groups, such as one group per input, appear as "Input 1", "Input 2" and so on, inside a heading for the whole set;
- **fields** inside them. Hover over a field's name to see its memory space, address and size.

Values outside any group are read as soon as the CDI has loaded.

Nodes whose CDI includes the standard user information (`<acdi/>`) get a **User info** card with **Name** and **Description**. The name is what JMRI and the Nodes list show for the node, so it's worth giving every board a name that says where it is, such as "Hilltown fascia".

### Kinds of field

| Field | How it looks |
|---|---|
| Number | A number box. The CDI's minimum and maximum, if any, are enforced. |
| Text | A text box, limited to the length the CDI allows. |
| Event ID | Eight hex bytes, such as `05.01.01.01.22.00.00.FF`. |
| Choice | A list of named values, such as *On* and *Off*. Picking one writes it straight away. |
| Decimal number | A number box that accepts fractions. |
| Action | A button that makes the node do something. The panel asks you to confirm first. |
| Blob | Raw data. Shown but not editable. |

## Changing a setting

1. Change the value. A **Write** button appears beside it.
2. Press **Write** (or Enter) to send it to the node. Press **↺** (or Esc) to undo your edit instead.

The value is checked against the CDI (minimum, maximum and length) before it is sent; if it doesn't fit, the field says why and nothing is written. After each write the panel tells the node that the update is complete, which is when most boards act on the new value.

**⟳** beside a field reads that one value again from the node.

The buttons at the top of the window:

- **Read all** reads every value in the CDI from the node, combining neighbouring fields into requests of up to 64 bytes.
- **Reload CDI** downloads the configuration description again, for example after a firmware update.
- **Restart node** restarts the board, for settings that only take effect after a restart. The panel asks first.

While requests are going out, the window shows "Working… *n* request(s) left". Requests go to the node one at a time, so a big **Read all** takes a little while.

### When something goes wrong

Errors are shown in the window. The common ones:

| Message | Meaning |
|---|---|
| *This node isn't on the bus right now.* | The node has gone (powered off, unplugged), or the panel isn't Online. |
| *node rejected the request (error …)* | The node refused the request. The code is the node's own error code. |
| *reading* or *writing space … at … failed (error …)* | The node couldn't read or write that address. |
| *node did not acknowledge in time* | The node didn't answer the request within 3 seconds. |
| *node did not reply in time* | The node accepted the request but didn't send the result in time. |
| *node is not on the bus* | The node left the bus while requests were waiting. |
| *The node's configuration description couldn't be read: …* | The CDI download failed, or the CDI isn't valid XML. |

## Trying it with the simulator

The simulator's layout controller has a CDI of its own, so you can try all of this without hardware. Connect to the simulator, open **Nodes**, and press **Configure** next to it. You'll find:

- **User info**: **Name** and **Description**. Writing the name changes the name the node reports to other programs, such as JMRI. The Nodes list and the window titles pick up the new name straight away.
- **Layout controller**, with a **Simulation** group. These settings take effect at once:
  - **Random occupancy**: *On* or *Off*. Turn it off to stop blocks flipping by themselves.
  - **Occupancy change period (ms)**: how often a random block changes. Default 3000.
  - **Turnout feedback delay (ms)**: how long a turnout takes to report its new position. Default 400.
- **Spare inputs**: four inputs, each with a **Description**, an **Active event** and an **Inactive event**. They do nothing but remember what you write, which makes them a handy place to practise.

## How it works

Configuration uses OpenLCB's *Memory Configuration* protocol, carried in *datagrams* (short addressed messages of up to 72 bytes). [How it works: node configuration](openlcb.md#node-configuration) shows the messages involved.
