//! A simulated OpenLCB layout for demos and tests: a layout controller node
//! (occupancy detectors and turnout drivers with feedback) and a command
//! station that creates a train node for every DCC address searched for.
//!
//! All simulated devices are genuine `rolcb::Node`s, so the dispatcher talks
//! to them with exactly the frames it would send to real hardware.
//!
//! # What is modelled
//!
//! * **Blocks.** Every block's `occupied_event` / `clear_event` is a
//!   producer whose state follows the simulated occupancy, so Identify
//!   Producer gets a truthful Valid/Invalid answer. [`Simulator::set_occupied`]
//!   changes a block and sends the matching PCER. With
//!   [`Simulator::random_occupancy`] a random block flips every few seconds.
//! * **Turnouts.** Command events are consumed. A turnout moves as soon as
//!   its command arrives; if it has feedback, the feedback producer reports
//!   the new position (state change plus PCER) after a short delay, like a
//!   slow-motion machine. (Unlike many real sensors, it reports again even
//!   when the position didn't change.) Identify Producer on a feedback
//!   event always answers with the current state.
//! * **Turntables.** The `cw_event` and `ccw_event` are consumed; each
//!   moves the bridge one track on. The bridge starts at the first track
//!   and takes [`TURNTABLE_STEP_MS`] per track; as it lines up with each
//!   track on the way, that track's feedback producer (if any) reports it.
//! * **Command station and trains.** A Traction search with `ALLOCATE`
//!   creates a train node for that DCC address; short and long addresses
//!   (the `DCC_LONG_ADDRESS` search flag) are distinct trains. Trains accept
//!   one controller at a time (refusing others), track speed and functions,
//!   answer queries, and ask their controller for a heartbeat every
//!   [`HEARTBEAT_MS`].
//!
//! * **Configuration.** The layout node serves a CDI ([`SIM_CDI`]) and
//!   answers Memory Configuration reads and writes. Its settings are live:
//!   random occupancy, how often a block changes, and the turnout feedback
//!   delay. Writing the node name or description (space `0xFB`) changes
//!   what it reports in SNIP. Writes are acknowledged with "reply pending"
//!   followed by a write reply, like most real nodes.
//!
//! * **A FemtoLCC.** A second node, node ID `02.01.57.FE.00.01`, poses as
//!   an Uncommon Models FemtoLCC board (see [`crate::femto`]) so it can be
//!   picked for a block or turnout in the editor: it answers SNIP, serves a
//!   cut-down copy of the board's CDI and keeps settings memory, but drives
//!   nothing. Output B's block events have been changed from the factory
//!   ones, output C is a turnout motor and servo board 1 channel 1 a servo
//!   turnout.
//!
//! Not modelled: CAN bandwidth, lost frames, consists and Controller
//! Changed notifications.

use std::collections::BTreeMap;

use rolcb::message::{protocol, Message};
use rolcb::traction::{search, Command, Reply, Speed};
use rolcb::{CanFrame, EventId, EventState, Node, NodeId, NodeInfo};

use crate::config::DATAGRAM_MEMORY_CONFIG;
use crate::dispatcher::Position;
use crate::layout::Layout;

mod femto_board;
use femto_board::SimFemto;
pub use femto_board::{SIM_FEMTO_CDI, SIM_FEMTO_NODE};

/// Trains ask their controller for a keep-alive this often.
pub const HEARTBEAT_MS: u64 = 10_000;
const FEEDBACK_DELAY_MS: u64 = 400;
/// How long a simulated turntable takes to move from one track to the next.
pub const TURNTABLE_STEP_MS: u64 = 500;
const OCCUPANCY_PERIOD_MS: u64 = 3_000;

/// The simulated layout node's CDI. Space `0xFD` holds the simulation
/// settings (see [the module docs](self)) and four spare inputs that do
/// nothing but remember their settings.
pub const SIM_CDI: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<cdi xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://openlcb.org/schema/cdi/1/1/cdi.xsd">
<identification>
<manufacturer>System4Dispatch</manufacturer>
<model>Simulated layout</model>
<hardwareVersion>Simulator</hardwareVersion>
<softwareVersion>1</softwareVersion>
</identification>
<acdi/>
<segment space="253">
<name>Layout controller</name>
<group>
<name>Simulation</name>
<description>How the simulated layout behaves. Changes take effect at once.</description>
<int size="1">
<name>Random occupancy</name>
<description>Flip a random block's occupancy every so often.</description>
<min>0</min><max>1</max><default>1</default>
<map><relation><property>0</property><value>Off</value></relation><relation><property>1</property><value>On</value></relation></map>
</int>
<int size="2">
<name>Occupancy change period (ms)</name>
<min>500</min><max>60000</max><default>3000</default>
</int>
<int size="2">
<name>Turnout feedback delay (ms)</name>
<description>How long a turnout takes to report its new position.</description>
<min>0</min><max>10000</max><default>400</default>
</int>
</group>
<group replication="4">
<name>Spare inputs</name>
<repname>Input</repname>
<string size="16"><name>Description</name></string>
<eventid><name>Active event</name></eventid>
<eventid><name>Inactive event</name></eventid>
</group>
</segment>
</cdi>
"#;
/// Size of the simulated node's configuration space (`0xFD`).
const SIM_CONFIG_SIZE: usize = 5 + 4 * (16 + 8 + 8);
/// Size of the ACDI user space (`0xFB`): version, name, description.
const SIM_USER_SIZE: usize = 128;

/// A simulated train node created by the command station.
pub struct SimTrain {
    /// DCC address.
    pub address: u16,
    /// True for a DCC long (extended) address.
    pub long_address: bool,
    /// The train's OpenLCB node.
    pub node: Node,
    /// The throttle in control, if any.
    pub controller: Option<NodeId>,
    controller_alias: Option<u16>,
    /// Last speed set.
    pub speed: Speed,
    /// Function states F0..=F28.
    pub functions: [bool; 29],
    /// Heartbeat answers (Noop commands) received.
    pub noops: u32,
    next_heartbeat: u64,
}

/// The simulated layout controller and command station. Sans-IO, like
/// [`Dispatcher`](crate::Dispatcher): feed it frames and time, collect
/// frames with [`Simulator::take_outgoing`].
pub struct Simulator {
    layout: Layout,
    node: Node,
    occupied: BTreeMap<String, bool>,
    turnouts: BTreeMap<String, Position>,
    pending_feedback: Vec<(u64, String, Position)>,
    /// Where each bridge is headed (track index), after any queued steps.
    turntables: BTreeMap<String, usize>,
    /// Steps under way: (time the bridge lines up, id, track index).
    turning: Vec<(u64, String, usize)>,
    trains: Vec<SimTrain>,
    femto: SimFemto,
    rng: u64,
    next_occupancy: u64,
    now: u64,
    /// Randomly flip block occupancy every few seconds.
    pub random_occupancy: bool,
    /// Configuration space `0xFD` (see [`SIM_CDI`]).
    config: Vec<u8>,
    /// ACDI user space `0xFB`.
    user: Vec<u8>,
}

fn info(model: &str) -> NodeInfo {
    NodeInfo {
        manufacturer: "System4Dispatch".into(),
        model: model.into(),
        software_version: env!("CARGO_PKG_VERSION").into(),
        ..Default::default()
    }
}

impl Simulator {
    /// Simulate `layout`, starting at `now` (milliseconds). All blocks
    /// start clear and all turnouts normal.
    pub fn new(layout: Layout, now: u64) -> Self {
        let mut s = Simulator {
            node: Node::new(NodeId::new(0x0501_0101_FF00), info("Simulated layout")),
            occupied: layout
                .blocks
                .iter()
                .map(|b| (b.id.clone(), false))
                .collect(),
            turnouts: layout
                .turnouts
                .iter()
                .map(|t| (t.id.clone(), Position::Normal))
                .collect(),
            turntables: layout
                .turntables
                .iter()
                .filter(|t| !t.tracks.is_empty())
                .map(|t| (t.id.clone(), 0))
                .collect(),
            turning: Vec::new(),
            layout,
            pending_feedback: Vec::new(),
            trains: Vec::new(),
            femto: SimFemto::new(now),
            rng: 0x2545_F491_4F6C_DD1D,
            next_occupancy: now + OCCUPANCY_PERIOD_MS,
            now,
            random_occupancy: true,
            config: sim_default_config(),
            user: {
                let mut u = vec![0; SIM_USER_SIZE];
                u[0] = 1;
                u
            },
        };
        s.node.set_protocols(
            protocol::EVENT_EXCHANGE
                | protocol::SNIP
                | protocol::DATAGRAM
                | protocol::MEMORY_CONFIGURATION
                | protocol::CDI
                | protocol::ACDI,
        );
        s.register_events();
        s.node.start(now);
        s
    }

    fn register_events(&mut self) {
        for b in &self.layout.blocks {
            let occ = self.occupied[&b.id];
            if let Some(e) = b.occupied_event {
                self.node.add_producer(e, valid(occ));
            }
            if let Some(e) = b.clear_event {
                self.node.add_producer(e, valid(!occ));
            }
        }
        for t in &self.layout.turnouts {
            let pos = self.turnouts[&t.id];
            if let Some(e) = t.normal_event {
                self.node.add_consumer(e, valid(pos == Position::Normal));
            }
            if let Some(e) = t.reverse_event {
                self.node.add_consumer(e, valid(pos == Position::Reverse));
            }
            if let Some(fb) = &t.feedback {
                if let Some(e) = fb.normal_event {
                    self.node.add_producer(e, valid(pos == Position::Normal));
                }
                if let Some(e) = fb.reverse_event {
                    self.node.add_producer(e, valid(pos == Position::Reverse));
                }
            }
        }
        for t in &self.layout.turntables {
            let at = self.turntables.get(&t.id).copied();
            for e in [t.cw_event, t.ccw_event].into_iter().flatten() {
                self.node.add_consumer(e, EventState::Unknown);
            }
            for (i, tr) in t.tracks.iter().enumerate() {
                if let Some(e) = tr.feedback_event {
                    self.node.add_producer(e, valid(at == Some(i)));
                }
            }
        }
    }

    /// The track a simulated turntable's bridge lines up with, or is
    /// turning towards.
    pub fn turntable_position(&self, id: &str) -> Option<usize> {
        self.turntables.get(id).copied()
    }

    /// The simulated position of a turnout.
    pub fn turnout_position(&self, id: &str) -> Option<Position> {
        self.turnouts.get(id).copied()
    }

    /// The simulated occupancy of a block.
    pub fn occupied(&self, id: &str) -> Option<bool> {
        self.occupied.get(id).copied()
    }

    /// The first train created for this DCC address (short or long). Use
    /// [`Simulator::find_train`] to tell short 3 from long 3.
    pub fn train(&self, address: u16) -> Option<&SimTrain> {
        self.trains.iter().find(|t| t.address == address)
    }

    /// The train for this DCC address and address kind.
    pub fn find_train(&self, address: u16, long_address: bool) -> Option<&SimTrain> {
        let long_address = long_address || address > 127;
        self.trains
            .iter()
            .find(|t| t.address == address && t.long_address == long_address)
    }

    /// Mutable access to a train, e.g. to make it send something in a test.
    pub fn train_mut(&mut self, address: u16, long_address: bool) -> Option<&mut SimTrain> {
        let long_address = long_address || address > 127;
        self.trains
            .iter_mut()
            .find(|t| t.address == address && t.long_address == long_address)
    }

    /// Every train created so far.
    pub fn trains(&self) -> &[SimTrain] {
        &self.trains
    }

    /// Change a block's occupancy and report it on the bus.
    pub fn set_occupied(&mut self, id: &str, occ: bool) {
        let Some(b) = self.layout.block(id).cloned() else {
            return;
        };
        self.occupied.insert(id.to_string(), occ);
        if let Some(e) = b.occupied_event {
            self.node.set_producer_state(e, valid(occ));
        }
        if let Some(e) = b.clear_event {
            self.node.set_producer_state(e, valid(!occ));
        }
        let report = if occ { b.occupied_event } else { b.clear_event };
        if let Some(e) = report {
            self.node.send(Message::Pcer(e));
        }
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Run timers: login, heartbeats, delayed feedback, random occupancy.
    pub fn poll(&mut self, now: u64) {
        self.now = now;
        self.node.poll(now);
        self.femto.node.poll(now);
        for t in &mut self.trains {
            t.node.poll(now);
            if t.controller.is_some() && now >= t.next_heartbeat {
                t.next_heartbeat = now + HEARTBEAT_MS;
                if let Some(dest) = t.controller_alias {
                    t.node.send(Message::TractionReply {
                        dest,
                        payload: Reply::Heartbeat { timeout_s: 10 }.encode(),
                    });
                }
            }
        }
        let due: Vec<_> = self
            .pending_feedback
            .iter()
            .filter(|p| p.0 <= now)
            .cloned()
            .collect();
        self.pending_feedback.retain(|p| p.0 > now);
        for (_, id, pos) in due {
            self.report_turnout(&id, pos);
        }
        let arrived: Vec<_> = self
            .turning
            .iter()
            .filter(|p| p.0 <= now)
            .cloned()
            .collect();
        self.turning.retain(|p| p.0 > now);
        for (_, id, dir) in arrived {
            self.report_turntable(&id, dir);
        }
        if self.random_occupancy && now >= self.next_occupancy && !self.layout.blocks.is_empty() {
            self.next_occupancy = now + self.config_u16(1);
            let i = (self.rand() % self.layout.blocks.len() as u64) as usize;
            let id = self.layout.blocks[i].id.clone();
            let occ = !self.occupied[&id];
            self.set_occupied(&id, occ);
        }
    }

    fn report_turnout(&mut self, id: &str, pos: Position) {
        let Some(t) = self.layout.turnout(id).cloned() else {
            return;
        };
        let Some(fb) = t.feedback else { return };
        let (on, off) = match pos {
            Position::Reverse => (fb.reverse_event, fb.normal_event),
            _ => (fb.normal_event, fb.reverse_event),
        };
        if let Some(e) = off {
            self.node.set_producer_state(e, EventState::Invalid);
        }
        if let Some(e) = on {
            self.node.set_producer_state(e, EventState::Valid);
            self.node.send(Message::Pcer(e));
        }
    }

    fn report_turntable(&mut self, id: &str, at: usize) {
        let Some(t) = self.layout.turntable(id).cloned() else {
            return;
        };
        for (i, tr) in t.tracks.iter().enumerate() {
            if let Some(e) = tr.feedback_event {
                self.node.set_producer_state(e, valid(i == at));
            }
        }
        if let Some(e) = t.tracks.get(at).and_then(|tr| tr.feedback_event) {
            self.node.send(Message::Pcer(e));
        }
    }

    fn turntable_command(&mut self, e: EventId, now: u64) {
        let hit = self.layout.turntables.iter().find_map(|t| {
            if t.cw_event == Some(e) {
                Some((t.clone(), 1))
            } else if t.ccw_event == Some(e) {
                Some((t.clone(), -1))
            } else {
                None
            }
        });
        let Some((t, step)) = hit else { return };
        let from = self.turntables.get(&t.id).copied().unwrap_or(0);
        let Some(to) = t.step_from(from, step) else {
            return;
        };
        self.turntables.insert(t.id.clone(), to);
        // The bridge leaves its track straight away...
        for tr in &t.tracks {
            if let Some(ev) = tr.feedback_event {
                self.node.set_producer_state(ev, EventState::Invalid);
            }
        }
        // ...and steps queue up behind each other.
        let start = self
            .turning
            .iter()
            .filter(|p| p.1 == t.id)
            .map(|p| p.0)
            .max()
            .unwrap_or(now)
            .max(now);
        self.turning.push((start + TURNTABLE_STEP_MS, t.id, to));
    }

    /// Process one frame from the bus.
    pub fn receive(&mut self, frame: &CanFrame, now: u64) {
        self.now = now;
        let rx = self.node.receive(frame, now);
        if let Some(r) = self.femto.node.receive(frame, now) {
            if let Message::Datagram { dest, data } = r.msg {
                if Some(dest) == self.femto.node.alias() {
                    self.femto.memory_config(r.src_alias, &data);
                }
            }
        }
        for t in &mut self.trains {
            if let Some(r) = t.node.receive(frame, now) {
                handle_train(t, r.src_alias, r.msg, now);
            }
        }
        let Some(rx) = rx else { return };
        match rx.msg {
            Message::Pcer(e) => {
                self.turnout_command(e, now);
                self.turntable_command(e, now);
            }
            Message::IdentifyProducer(e) if search::is_search_event(e) => self.train_search(e, now),
            Message::Datagram { dest, data } if Some(dest) == self.node.alias() => {
                self.memory_config(rx.src_alias, &data)
            }
            _ => {}
        }
    }

    fn config_u16(&self, at: usize) -> u64 {
        u16::from_be_bytes([self.config[at], self.config[at + 1]]) as u64
    }

    /// Answer a Memory Configuration datagram from `src`.
    fn memory_config(&mut self, src: u16, d: &[u8]) {
        let reject = |s: &mut Self| {
            s.node.send(Message::DatagramRejected {
                dest: src,
                code: 0x1043,
            });
        };
        if d.len() < 2 || d[0] != DATAGRAM_MEMORY_CONFIG {
            return reject(self);
        }
        if matches!(d[1], 0xA8 | 0xA9) {
            // Update complete / reset: nothing to do.
            self.node.send(Message::DatagramReceivedOk {
                dest: src,
                flags: 0,
            });
            return;
        }
        if d.len() < 6 {
            return reject(self);
        }
        let cmd = d[1] & 0xFC;
        let bits = d[1] & 0x03;
        let addr = u32::from_be_bytes([d[2], d[3], d[4], d[5]]) as usize;
        let (space, rest) = match bits {
            0 if d.len() >= 7 => (d[6], &d[7..]),
            0 => return reject(self),
            b => (0xFC | b, &d[6..]),
        };
        // The reply echoes the request's command flags, address and space.
        let mut head = vec![DATAGRAM_MEMORY_CONFIG, 0];
        head.extend_from_slice(&d[2..6]);
        if bits == 0 {
            head.push(space);
        }
        let reply = |s: &mut Self, cmd: u8, extra: &[u8]| {
            let mut r = head.clone();
            r[1] = cmd | bits;
            r.extend_from_slice(extra);
            s.node.send(Message::Datagram { dest: src, data: r });
        };
        if cmd == 0x40 {
            self.node.send(Message::DatagramReceivedOk {
                dest: src,
                flags: 0,
            });
            let count = rest.first().copied().unwrap_or(0).min(64) as usize;
            self.config[0] = self.random_occupancy as u8;
            let mut cdi = SIM_CDI.as_bytes().to_vec();
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
                    reply(self, 0x50, &data);
                }
                Some(_) => reply(self, 0x58, &[0x10, 0x82]),
                None => reply(self, 0x58, &[0x10, 0x81]),
            }
        } else if cmd == 0x00 {
            // "Reply pending, within 2 s", then the write reply.
            self.node.send(Message::DatagramReceivedOk {
                dest: src,
                flags: 0x81,
            });
            self.config[0] = self.random_occupancy as u8;
            let mem = match space {
                0xFD => Some(&mut self.config),
                0xFB => Some(&mut self.user),
                _ => None,
            };
            match mem {
                Some(m) if addr + rest.len() <= m.len() => {
                    m[addr..addr + rest.len()].copy_from_slice(rest);
                    reply(self, 0x10, &[]);
                    self.apply_config();
                }
                Some(_) => reply(self, 0x18, &[0x10, 0x82]),
                None => reply(self, 0x18, &[0x10, 0x83]),
            }
        } else {
            reject(self);
        }
    }

    /// Make the simulation follow its configuration memory.
    fn apply_config(&mut self) {
        self.random_occupancy = self.config[0] != 0;
        let text = |b: &[u8]| {
            let end = b.iter().position(|x| *x == 0).unwrap_or(b.len());
            String::from_utf8_lossy(&b[..end]).into_owned()
        };
        let mut info = self.node.info().clone();
        info.user_name = text(&self.user[1..64]);
        info.user_description = text(&self.user[64..]);
        self.node.set_info(info);
    }

    fn turnout_command(&mut self, e: EventId, now: u64) {
        let hit = self.layout.turnouts.iter().find_map(|t| {
            if t.normal_event == Some(e) {
                Some((t.clone(), Position::Normal))
            } else if t.reverse_event == Some(e) {
                Some((t.clone(), Position::Reverse))
            } else {
                None
            }
        });
        let Some((t, pos)) = hit else { return };
        self.turnouts.insert(t.id.clone(), pos);
        for (ev, p) in [
            (t.normal_event, Position::Normal),
            (t.reverse_event, Position::Reverse),
        ] {
            if let Some(ev) = ev {
                self.node.set_consumer_state(ev, valid(p == pos));
            }
        }
        if t.has_feedback() {
            self.pending_feedback
                .push((now + self.config_u16(3), t.id, pos));
        }
    }

    fn train_search(&mut self, e: EventId, now: u64) {
        let Some((address, flags)) = search::decode(e) else {
            return;
        };
        let address = address as u16;
        let long_address = flags & search::DCC_LONG_ADDRESS != 0 || address > 127;
        if let Some(t) = self
            .trains
            .iter_mut()
            .find(|t| t.address == address && t.long_address == long_address)
        {
            t.node.send(Message::ProducerIdentified {
                event: e,
                state: EventState::Valid,
            });
            return;
        }
        // 05.01.01.FE.xx.xx: address in the low 14 bits, bit 15 = long address.
        let id = NodeId::new(0x0501_01FE_0000 | address as u64 | (long_address as u64) << 15);
        let kind = if long_address { " (long)" } else { "" };
        let mut node = Node::new(id, info(&format!("Simulated train {address}{kind}")));
        node.set_protocols(
            rolcb::message::protocol::EVENT_EXCHANGE
                | rolcb::message::protocol::SNIP
                | rolcb::message::protocol::TRACTION_CONTROL,
        );
        node.add_producer(EventId::IS_TRAIN, EventState::Valid);
        node.start(now);
        node.send(Message::ProducerIdentified {
            event: e,
            state: EventState::Valid,
        });
        self.trains.push(SimTrain {
            address,
            long_address,
            node,
            controller: None,
            controller_alias: None,
            speed: Speed::STOP_FORWARD,
            functions: [false; 29],
            noops: 0,
            next_heartbeat: u64::MAX,
        });
    }

    /// Frames to put on the bus.
    pub fn take_outgoing(&mut self) -> Vec<CanFrame> {
        let mut out = self.node.take_outgoing();
        out.extend(self.femto.node.take_outgoing());
        for t in &mut self.trains {
            out.extend(t.node.take_outgoing());
        }
        out
    }
}

fn sim_default_config() -> Vec<u8> {
    let mut c = vec![0; SIM_CONFIG_SIZE];
    c[0] = 1;
    c[1..3].copy_from_slice(&(OCCUPANCY_PERIOD_MS as u16).to_be_bytes());
    c[3..5].copy_from_slice(&(FEEDBACK_DELAY_MS as u16).to_be_bytes());
    c
}

fn valid(b: bool) -> EventState {
    if b {
        EventState::Valid
    } else {
        EventState::Invalid
    }
}

fn handle_train(t: &mut SimTrain, src: u16, msg: Message, now: u64) {
    let Message::TractionCommand { dest, payload } = msg else {
        return;
    };
    if Some(dest) != t.node.alias() {
        return;
    }
    let reply = |t: &mut SimTrain, r: Reply| {
        t.node.send(Message::TractionReply {
            dest: src,
            payload: r.encode(),
        });
    };
    match Command::decode(&payload) {
        Command::AssignController { controller, .. } => {
            let ok = t.controller.is_none() || t.controller == Some(controller);
            if ok {
                t.controller = Some(controller);
                t.controller_alias = Some(src);
                t.next_heartbeat = now + HEARTBEAT_MS;
            }
            reply(
                t,
                Reply::AssignController {
                    result: if ok { 0 } else { 1 },
                },
            );
        }
        Command::ReleaseController { controller, .. } => {
            if t.controller == Some(controller) {
                t.controller = None;
                t.controller_alias = None;
                t.next_heartbeat = u64::MAX;
            }
        }
        Command::SetSpeed(s) => t.speed = s,
        Command::EmergencyStop => {
            t.speed = if t.speed.is_reverse() {
                Speed::STOP_REVERSE
            } else {
                Speed::STOP_FORWARD
            }
        }
        Command::SetFunction { addr, value } => {
            if let Some(f) = t.functions.get_mut(addr as usize) {
                *f = value != 0;
            }
        }
        Command::QuerySpeed => {
            let r = Reply::QuerySpeed {
                set_speed: t.speed,
                estop: false,
                commanded: t.speed,
                actual: t.speed,
            };
            reply(t, r);
        }
        Command::QueryFunction { addr } => {
            let value = t.functions.get(addr as usize).copied().unwrap_or(false) as u16;
            reply(t, Reply::QueryFunction { addr, value });
        }
        Command::Noop => {
            t.noops += 1;
            t.next_heartbeat = now + HEARTBEAT_MS;
        }
        _ => {}
    }
}
