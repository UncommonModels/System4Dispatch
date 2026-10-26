//! A simulated FemtoLCC board (see [`crate::femto`]): enough of one to be
//! picked for a block or turnout in the editor. It answers Simple Node
//! Information as the real board does, serves a cut-down copy of its CDI and
//! keeps a configuration memory that can be read and written. It doesn't
//! drive anything.

use rolcb::message::{protocol, Message};
use rolcb::{Node, NodeId, NodeInfo};

use crate::cdi::Cdi;
use crate::config::DATAGRAM_MEMORY_CONFIG;
use crate::femto::{self, Output};

/// The simulated board's node ID: Uncommon Models' prefix, then FE.00.01.
pub const SIM_FEMTO_NODE: u64 = 0x0201_57FE_0001;

/// The settings segment's origin, as on the real board.
const ORIGIN: usize = 128;

/// The board's CDI, cut down to the outputs and servo boards.
pub const SIM_FEMTO_CDI: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<cdi>
<identification><manufacturer>Uncommon Models</manufacturer><model>FemtoLCC</model>
<hardwareVersion>Simulated</hardwareVersion></identification>
<acdi/>
<segment space="253" origin="128"><name>Settings</name>
<description>A simulated FemtoLCC: the settings can be read and written, but nothing is driven.</description>
<group replication="4"><name>Outputs</name><repname>Output</repname>
  <string size="24"><name>Description</name><description>What this output drives.</description></string>
  <int size="1"><name>Use</name><default>0</default><map>
    <relation><property>0</property><value>Unused</value></relation>
    <relation><property>1</property><value>Track block</value></relation>
    <relation><property>2</property><value>Turnout motor</value></relation></map></int>
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
<group replication="2"><name>Servo and light boards</name><repname>Board</repname>
  <int size="1"><name>Board type</name><map>
    <relation><property>0</property><value>None</value></relation>
    <relation><property>1</property><value>PCA9685</value></relation></map></int>
  <group replication="16"><name>Channels</name><repname>Channel</repname>
    <int size="1"><name>Use</name><map>
      <relation><property>0</property><value>Unused</value></relation>
      <relation><property>1</property><value>Servo turnout, closed at power-on</value></relation>
      <relation><property>2</property><value>Servo turnout, thrown at power-on</value></relation>
      <relation><property>3</property><value>Servo turnout, left alone</value></relation>
      <relation><property>4</property><value>Light, off at power-on</value></relation></map></int>
    <eventid><name>Throw / light on</name></eventid>
    <eventid><name>Close / light off</name></eventid>
    <eventid><name>Thrown</name></eventid>
    <eventid><name>Closed</name></eventid>
  </group>
</group>
</segment>
</cdi>"#;

pub struct SimFemto {
    pub node: Node,
    /// Configuration space `0xFD`, from address 0 (the settings start at
    /// [`ORIGIN`]).
    config: Vec<u8>,
    /// ACDI user space `0xFB`.
    user: Vec<u8>,
}

impl SimFemto {
    /// A board as set up on a layout: output A a track block, output B a
    /// track block whose events were changed from the factory ones, output
    /// C a turnout motor, output D unused, and channel 1 of servo board 1 a
    /// servo turnout.
    pub fn new(now: u64) -> SimFemto {
        let id = NodeId::new(SIM_FEMTO_NODE);
        let mut node = Node::new(
            id,
            NodeInfo {
                manufacturer: femto::MANUFACTURER.into(),
                model: femto::MODEL.into(),
                hardware_version: "Simulated".into(),
                software_version: env!("CARGO_PKG_VERSION").into(),
                user_name: "Simulated FemtoLCC".into(),
                ..Default::default()
            },
        );
        node.set_protocols(
            protocol::EVENT_EXCHANGE
                | protocol::SNIP
                | protocol::DATAGRAM
                | protocol::MEMORY_CONFIGURATION
                | protocol::CDI
                | protocol::ACDI,
        );
        let mut b = SimFemto {
            node,
            config: Vec::new(),
            user: {
                let mut u = vec![0; 128];
                u[0] = 1;
                u[1..19].copy_from_slice(b"Simulated FemtoLCC");
                u
            },
        };
        b.config = b.factory_settings(id);
        b.node.start(now);
        b
    }

    fn factory_settings(&self, id: NodeId) -> Vec<u8> {
        let cdi = Cdi::parse(SIM_FEMTO_CDI).expect("simulated FemtoLCC CDI");
        let end = cdi
            .fields()
            .iter()
            .map(|f| (f.address + f.size) as usize)
            .max()
            .unwrap_or(ORIGIN);
        let mut mem = vec![0; end];
        let mut put = |addr: u32, bytes: &[u8]| {
            mem[addr as usize..addr as usize + bytes.len()].copy_from_slice(bytes);
        };
        for n in 0..femto::OUTPUTS {
            let (occ, clr) = femto::default_block_events(id, n);
            if let Some(f) = femto::block_fields(&cdi, n) {
                let (occ, clr) = if n == 1 {
                    // Output B was given events of the layout's own.
                    (
                        rolcb::EventId::new(0x0501_0101_0142_0001),
                        rolcb::EventId::new(0x0501_0101_0142_0000),
                    )
                } else {
                    (occ, clr)
                };
                put(f.events[0].address, &occ.as_u64().to_be_bytes());
                put(f.events[1].address, &clr.as_u64().to_be_bytes());
                put(f.role.address, &[[1, 1, 2, 0][n as usize]]);
            }
        }
        for out in Output::for_turnouts() {
            if let Some(f) = femto::turnout_fields(&cdi, out) {
                let t = femto::default_turnout_events(id, out);
                for (field, e) in f.events.iter().zip([
                    t.normal,
                    t.reverse,
                    t.normal_feedback,
                    t.reverse_feedback,
                ]) {
                    put(field.address, &e.as_u64().to_be_bytes());
                }
                if out
                    == (Output::Servo {
                        board: 0,
                        channel: 0,
                    })
                {
                    put(f.role.address, &[3]);
                }
            }
        }
        mem
    }

    /// Answer a Memory Configuration datagram from `src`.
    pub fn memory_config(&mut self, src: u16, d: &[u8]) {
        let reject = |node: &mut Node| {
            node.send(Message::DatagramRejected {
                dest: src,
                code: 0x1043,
            });
        };
        if d.len() < 6 || d[0] != DATAGRAM_MEMORY_CONFIG {
            if d.len() >= 2 && d[0] == DATAGRAM_MEMORY_CONFIG && matches!(d[1], 0xA8 | 0xA9) {
                self.node.send(Message::DatagramReceivedOk {
                    dest: src,
                    flags: 0,
                });
                return;
            }
            return reject(&mut self.node);
        }
        let cmd = d[1] & 0xFC;
        let bits = d[1] & 0x03;
        let addr = u32::from_be_bytes([d[2], d[3], d[4], d[5]]) as usize;
        let (space, rest) = match bits {
            0 if d.len() >= 7 => (d[6], &d[7..]),
            0 => return reject(&mut self.node),
            b => (0xFC | b, &d[6..]),
        };
        let mut head = vec![DATAGRAM_MEMORY_CONFIG, 0];
        head.extend_from_slice(&d[2..6]);
        if bits == 0 {
            head.push(space);
        }
        let reply = |node: &mut Node, cmd: u8, extra: &[u8]| {
            let mut r = head.clone();
            r[1] = cmd | bits;
            r.extend_from_slice(extra);
            node.send(Message::Datagram { dest: src, data: r });
        };
        match cmd {
            0x40 => {
                self.node.send(Message::DatagramReceivedOk {
                    dest: src,
                    flags: 0,
                });
                let count = rest.first().copied().unwrap_or(0).min(64) as usize;
                let mut cdi = SIM_FEMTO_CDI.as_bytes().to_vec();
                cdi.push(0);
                let mem: Option<&[u8]> = match space {
                    0xFF => Some(&cdi),
                    0xFD => Some(&self.config),
                    0xFB => Some(&self.user),
                    _ => None,
                };
                match mem {
                    Some(m) if addr < m.len() => {
                        let data = m[addr..(addr + count).min(m.len())].to_vec();
                        reply(&mut self.node, 0x50, &data);
                    }
                    Some(_) => reply(&mut self.node, 0x58, &[0x10, 0x82]),
                    None => reply(&mut self.node, 0x58, &[0x10, 0x81]),
                }
            }
            0x00 => {
                self.node.send(Message::DatagramReceivedOk {
                    dest: src,
                    flags: 0x81,
                });
                let mem = match space {
                    0xFD => Some(&mut self.config),
                    0xFB => Some(&mut self.user),
                    _ => None,
                };
                match mem {
                    Some(m) if addr + rest.len() <= m.len() => {
                        m[addr..addr + rest.len()].copy_from_slice(rest);
                        reply(&mut self.node, 0x10, &[]);
                    }
                    Some(_) => reply(&mut self.node, 0x18, &[0x10, 0x82]),
                    None => reply(&mut self.node, 0x18, &[0x10, 0x83]),
                }
            }
            _ => reject(&mut self.node),
        }
    }
}
