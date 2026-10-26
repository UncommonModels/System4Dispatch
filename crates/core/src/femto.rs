//! Uncommon Models' FemtoLCC: an LCC node with four outputs, A to D, each
//! a track block or a turnout motor, plus up to two servo boards of sixteen
//! channels that can work turnouts.
//!
//! The board takes its default event IDs from its node ID: the node ID in
//! the top six bytes and a fixed suffix in the low two. The suffixes are
//! part of the board's published interface (the firmware never renumbers
//! them), so a block's or turnout's events can be filled in from the node
//! ID alone. For output A–D (`n` = 0–3) and servo channel `i` = 16 × board
//! + channel (all counted from 0):
//!
//! | Event | Output | Servo channel |
//! |---|---|---|
//! | Occupied | `0x0300` + n | |
//! | Clear | `0x0310` + n | |
//! | Throw (reverse) | `0x0400` + n | `0x0B00` + i |
//! | Close (normal) | `0x0410` + n | `0x0C00` + i |
//! | Thrown | `0x0420` + n | `0x0D00` + i |
//! | Closed | `0x0430` + n | `0x0E00` + i |
//!
//! Those are the factory settings. Someone may since have changed them in
//! JMRI, so when the board can be reached the configured values are read
//! from it instead: [`block_fields`] and [`turnout_fields`] find them in
//! the board's CDI.

use rolcb::{EventId, NodeId};

use crate::cdi::{Cdi, Field, Item};

/// The manufacturer in the board's Simple Node Information.
pub const MANUFACTURER: &str = "Uncommon Models";
/// The model in the board's Simple Node Information.
pub const MODEL: &str = "FemtoLCC";
/// How many main outputs a board has.
pub const OUTPUTS: u8 = 4;
/// How many servo boards a board can drive.
pub const SERVO_BOARDS: u8 = 2;
/// Channels on each servo board.
pub const SERVO_CHANNELS: u8 = 16;

/// True if a node's Simple Node Information says it's a FemtoLCC.
pub fn is_femto(manufacturer: &str, model: &str) -> bool {
    manufacturer == MANUFACTURER && model == MODEL
}

/// Something on a board that a block or turnout can be wired to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Output {
    /// Output A–D (0–3).
    Main(u8),
    /// A servo board channel, both counted from 0.
    Servo {
        /// The servo board (0 or 1).
        board: u8,
        /// The channel on it (0–15).
        channel: u8,
    },
}

impl Output {
    /// The outputs that can detect a block: A–D.
    pub fn for_blocks() -> Vec<Output> {
        (0..OUTPUTS).map(Output::Main).collect()
    }

    /// The outputs that can work a turnout: A–D, then every servo channel.
    pub fn for_turnouts() -> Vec<Output> {
        let mut v = Output::for_blocks();
        for board in 0..SERVO_BOARDS {
            for channel in 0..SERVO_CHANNELS {
                v.push(Output::Servo { board, channel });
            }
        }
        v
    }

    /// How it's saved in a layout file: `"A"`–`"D"`, or `"S<board>.<channel>"`
    /// counted from 1 (`"S1.1"` is the first channel of the first board).
    ///
    /// ```
    /// use s4d_core::femto::Output;
    ///
    /// assert_eq!(Output::Main(1).code(), "B");
    /// assert_eq!(Output::Servo { board: 1, channel: 4 }.code(), "S2.5");
    /// assert_eq!(Output::parse("s2.5"), Some(Output::Servo { board: 1, channel: 4 }));
    /// assert_eq!(Output::parse("E"), None);
    /// ```
    pub fn code(self) -> String {
        match self {
            Output::Main(n) => ((b'A' + n) as char).to_string(),
            Output::Servo { board, channel } => format!("S{}.{}", board + 1, channel + 1),
        }
    }

    /// Read [`code`](Output::code) back.
    pub fn parse(s: &str) -> Option<Output> {
        let s = s.trim().to_ascii_uppercase();
        if let Some(rest) = s.strip_prefix('S') {
            let (b, c) = rest.split_once('.')?;
            let (b, c) = (b.parse::<u8>().ok()?, c.parse::<u8>().ok()?);
            let ok = (1..=SERVO_BOARDS).contains(&b) && (1..=SERVO_CHANNELS).contains(&c);
            return ok.then_some(Output::Servo {
                board: b - 1,
                channel: c - 1,
            });
        }
        let mut chars = s.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return None;
        };
        let n = (c as u8).checked_sub(b'A')?;
        (n < OUTPUTS).then_some(Output::Main(n))
    }

    /// A name for lists: `"Output B"`, `"Servo board 2, channel 5"`.
    pub fn label(self) -> String {
        match self {
            Output::Main(_) => format!("Output {}", self.code()),
            Output::Servo { board, channel } => {
                format!("Servo board {}, channel {}", board + 1, channel + 1)
            }
        }
    }
}

fn event(node: NodeId, suffix: u16) -> EventId {
    EventId::new(node.as_u64() << 16 | suffix as u64)
}

/// The factory (occupied, clear) events of block output `output` (0–3) on
/// board `node`.
///
/// ```
/// use s4d_core::femto::default_block_events;
///
/// let node = "02.01.57.A7.CD.94".parse().unwrap();
/// let (occ, clr) = default_block_events(node, 1); // output B
/// assert_eq!(occ.to_string(), "02.01.57.A7.CD.94.03.01");
/// assert_eq!(clr.to_string(), "02.01.57.A7.CD.94.03.11");
/// ```
pub fn default_block_events(node: NodeId, output: u8) -> (EventId, EventId) {
    let n = output as u16;
    (event(node, 0x0300 + n), event(node, 0x0310 + n))
}

/// A turnout's four events on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnoutEvents {
    /// Close: sets the turnout normal.
    pub normal: EventId,
    /// Throw: sets it reverse.
    pub reverse: EventId,
    /// Closed: sent once it has been set normal.
    pub normal_feedback: EventId,
    /// Thrown: sent once it has been set reverse.
    pub reverse_feedback: EventId,
}

/// The factory events of a turnout on `output` of board `node`.
///
/// ```
/// use s4d_core::femto::{default_turnout_events, Output};
///
/// let node = "02.01.57.A7.CD.94".parse().unwrap();
/// let t = default_turnout_events(node, Output::Main(2));
/// assert_eq!(t.reverse.to_string(), "02.01.57.A7.CD.94.04.02");
/// assert_eq!(t.normal_feedback.to_string(), "02.01.57.A7.CD.94.04.32");
/// let s = default_turnout_events(node, Output::Servo { board: 1, channel: 2 });
/// assert_eq!(s.normal.to_string(), "02.01.57.A7.CD.94.0C.12");
/// ```
pub fn default_turnout_events(node: NodeId, output: Output) -> TurnoutEvents {
    let (throw, close, thrown, closed) = match output {
        Output::Main(n) => {
            let n = n as u16;
            (0x0400 + n, 0x0410 + n, 0x0420 + n, 0x0430 + n)
        }
        Output::Servo { board, channel } => {
            let i = (board as u16) * SERVO_CHANNELS as u16 + channel as u16;
            (0x0B00 + i, 0x0C00 + i, 0x0D00 + i, 0x0E00 + i)
        }
    };
    TurnoutEvents {
        normal: event(node, close),
        reverse: event(node, throw),
        normal_feedback: event(node, closed),
        reverse_feedback: event(node, thrown),
    }
}

/// Where one output's settings live in the board's configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputFields {
    /// What the output is used for.
    pub role: Field,
    /// The values of `role` that suit what it's being picked for.
    pub suitable: Vec<u8>,
    /// The value to set `role` to, to make it suitable.
    pub make_suitable: u8,
    /// The event fields, in the order of the events they correspond to:
    /// (occupied, clear) for a block; (normal, reverse, normal feedback,
    /// reverse feedback) for a turnout.
    pub events: Vec<Field>,
}

/// Find output `output`'s (0–3) block settings in a FemtoLCC's CDI: the
/// "Use" field of group "Output N", and "Occupied" and "Clear" in its
/// "Track block" group.
pub fn block_fields(cdi: &Cdi, output: u8) -> Option<OutputFields> {
    let out = output_group(cdi, Output::Main(output))?;
    let block = find_group(out, "Track block")?;
    Some(OutputFields {
        role: field(out, "Use")?,
        suitable: vec![1],
        make_suitable: 1,
        events: vec![field(block, "Occupied")?, field(block, "Clear")?],
    })
}

/// Find the turnout settings of `output` in a FemtoLCC's CDI: for output
/// A–D, the "Turnout motor" group of "Output N"; for a servo channel,
/// "Channel M" of "Board N" under "Servo and light boards".
pub fn turnout_fields(cdi: &Cdi, output: Output) -> Option<OutputFields> {
    let out = output_group(cdi, output)?;
    Some(match output {
        Output::Main(_) => {
            let motor = find_group(out, "Turnout motor")?;
            OutputFields {
                role: field(out, "Use")?,
                suitable: vec![2],
                make_suitable: 2,
                events: vec![
                    field(motor, "Close")?,
                    field(motor, "Throw")?,
                    field(motor, "Closed")?,
                    field(motor, "Thrown")?,
                ],
            }
        }
        Output::Servo { .. } => OutputFields {
            role: field(out, "Use")?,
            suitable: vec![1, 2, 3],
            make_suitable: 3,
            events: vec![
                field(out, "Close / light off")?,
                field(out, "Throw / light on")?,
                field(out, "Closed")?,
                field(out, "Thrown")?,
            ],
        },
    })
}

/// The group holding one output's settings.
fn output_group(cdi: &Cdi, output: Output) -> Option<&[Item]> {
    cdi.segments.iter().find_map(|s| match output {
        Output::Main(n) => find_group(&s.items, &format!("Output {}", n + 1)),
        Output::Servo { board, channel } => {
            let boards = find_group(&s.items, "Servo and light boards")?;
            let board = find_group(boards, &format!("Board {}", board + 1))?;
            find_group(board, &format!("Channel {}", channel + 1))
        }
    })
}

fn field(items: &[Item], name: &str) -> Option<Field> {
    items.iter().find_map(|i| match i {
        Item::Field(f) if f.name == name => Some(f.clone()),
        _ => None,
    })
}

fn find_group<'a>(items: &'a [Item], name: &str) -> Option<&'a [Item]> {
    items.iter().find_map(|i| match i {
        Item::Group(g) if g.name == name => Some(g.items.as_slice()),
        Item::Group(g) => find_group(&g.items, name),
        Item::Field(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the board's CDI, cut down. The I/O boards come before
    /// the servo boards and use the same "Board N" names.
    const CDI: &str = r#"<?xml version="1.0"?><cdi>
        <identification><manufacturer>Uncommon Models</manufacturer><model>FemtoLCC</model></identification>
        <acdi/>
        <segment space="253" origin="128"><name>Settings</name>
          <group replication="4"><name>Outputs</name><repname>Output</repname>
            <string size="24"><name>Description</name></string>
            <int size="1"><name>Use</name></int>
            <eventid><name>Fault</name></eventid>
            <group><name>Track block</name>
              <int size="1"><name>At power-on</name></int>
              <eventid><name>Power on (DC)</name></eventid>
              <eventid><name>Occupied</name></eventid>
              <eventid><name>Clear</name></eventid>
            </group>
            <group><name>Turnout motor</name>
              <eventid><name>Throw</name></eventid>
              <eventid><name>Close</name></eventid>
              <eventid><name>Thrown</name></eventid>
              <eventid><name>Closed</name></eventid>
            </group>
          </group>
          <group replication="2"><name>I/O boards</name><repname>Board</repname>
            <int size="1"><name>Board type</name></int>
            <group replication="2"><name>Lines</name><repname>Line</repname>
              <int size="1"><name>Mode</name></int>
            </group>
          </group>
          <group replication="2"><name>Servo and light boards</name><repname>Board</repname>
            <int size="1"><name>Board type</name></int>
            <group replication="16"><name>Channels</name><repname>Channel</repname>
              <int size="1"><name>Use</name></int>
              <eventid><name>Throw / light on</name></eventid>
              <eventid><name>Close / light off</name></eventid>
              <eventid><name>Thrown</name></eventid>
              <eventid><name>Closed</name></eventid>
            </group>
          </group>
        </segment></cdi>"#;

    #[test]
    fn finds_each_outputs_block_fields() {
        let cdi = Cdi::parse(CDI).unwrap();
        // Each output is 24 + 1 + 8 + (1 + 8 + 8 + 8) + 4 × 8 = 90 bytes.
        let a = block_fields(&cdi, 0).unwrap();
        assert_eq!(a.role.address, 152);
        assert_eq!((a.events[0].address, a.events[1].address), (170, 178));
        let d = block_fields(&cdi, 3).unwrap();
        assert_eq!(d.events[0].address, 170 + 3 * 90);
        assert!(block_fields(&cdi, 4).is_none());
    }

    #[test]
    fn finds_turnout_fields_on_outputs_and_servo_boards() {
        let cdi = Cdi::parse(CDI).unwrap();
        let b = turnout_fields(&cdi, Output::Main(1)).unwrap();
        // Close, Throw, Closed, Thrown: Throw is first in the group.
        let motor = 128 + 90 + 24 + 1 + 8 + 25;
        let addrs: Vec<u32> = b.events.iter().map(|f| f.address).collect();
        assert_eq!(addrs, vec![motor + 8, motor, motor + 24, motor + 16]);
        assert_eq!(b.suitable, vec![2]);
        // Servo board 2, channel 3: past the outputs, both I/O boards, the
        // first servo board and two channels.
        let s = turnout_fields(
            &cdi,
            Output::Servo {
                board: 1,
                channel: 2,
            },
        )
        .unwrap();
        let servo_board = 1 + 16 * 33;
        let start = 128 + 4 * 90 + 2 * 3 + servo_board + 1 + 2 * 33;
        assert_eq!(s.role.address, start);
        assert_eq!(s.events[1].address, start + 1);
        assert!(turnout_fields(
            &cdi,
            Output::Servo {
                board: 2,
                channel: 0
            }
        )
        .is_none());
    }

    #[test]
    fn outputs_for_each_use() {
        assert_eq!(Output::for_blocks().len(), 4);
        assert_eq!(Output::for_turnouts().len(), 4 + 32);
        for o in Output::for_turnouts() {
            assert_eq!(Output::parse(&o.code()), Some(o));
        }
    }
}
