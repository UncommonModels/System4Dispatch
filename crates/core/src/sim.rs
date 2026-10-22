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
//! * **Command station and trains.** A Traction search with `ALLOCATE`
//!   creates a train node for that DCC address; short and long addresses
//!   (the `DCC_LONG_ADDRESS` search flag) are distinct trains. Trains accept
//!   one controller at a time (refusing others), track speed and functions,
//!   answer queries, and ask their controller for a heartbeat every
//!   [`HEARTBEAT_MS`].
//!
//! Not modelled: CAN bandwidth, lost frames, consists, Memory
//! Configuration and Controller Changed notifications.

use std::collections::BTreeMap;

use rolcb::message::Message;
use rolcb::traction::{search, Command, Reply, Speed};
use rolcb::{CanFrame, EventId, EventState, Node, NodeId, NodeInfo};

use crate::dispatcher::Position;
use crate::layout::Layout;

/// Trains ask their controller for a keep-alive this often.
pub const HEARTBEAT_MS: u64 = 10_000;
const FEEDBACK_DELAY_MS: u64 = 400;
const OCCUPANCY_PERIOD_MS: u64 = 3_000;

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
    trains: Vec<SimTrain>,
    rng: u64,
    next_occupancy: u64,
    now: u64,
    /// Randomly flip block occupancy every few seconds.
    pub random_occupancy: bool,
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
            layout,
            pending_feedback: Vec::new(),
            trains: Vec::new(),
            rng: 0x2545_F491_4F6C_DD1D,
            next_occupancy: now + OCCUPANCY_PERIOD_MS,
            now,
            random_occupancy: true,
        };
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
        if self.random_occupancy && now >= self.next_occupancy && !self.layout.blocks.is_empty() {
            self.next_occupancy = now + OCCUPANCY_PERIOD_MS;
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

    /// Process one frame from the bus.
    pub fn receive(&mut self, frame: &CanFrame, now: u64) {
        self.now = now;
        let rx = self.node.receive(frame, now);
        for t in &mut self.trains {
            if let Some(r) = t.node.receive(frame, now) {
                handle_train(t, r.src_alias, r.msg, now);
            }
        }
        let Some(rx) = rx else { return };
        match rx.msg {
            Message::Pcer(e) => self.turnout_command(e, now),
            Message::IdentifyProducer(e) if search::is_search_event(e) => self.train_search(e, now),
            _ => {}
        }
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
                .push((now + FEEDBACK_DELAY_MS, t.id, pos));
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
        for t in &mut self.trains {
            out.extend(t.node.take_outgoing());
        }
        out
    }
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
