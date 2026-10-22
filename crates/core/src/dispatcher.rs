//! The dispatcher: one OpenLCB node that tracks block occupancy and turnout
//! positions for a [`Layout`], throws turnouts, and runs throttles.
//!
//! Like `rolcb::Node` it is sans-IO: feed it frames and time, collect frames
//! to send with [`Dispatcher::take_outgoing`].

use std::collections::{BTreeMap, VecDeque};

use rolcb::message::Message;
use rolcb::mti::Mti;
use rolcb::traction::{search, Command, Reply, Speed};
use rolcb::{CanFrame, EventId, EventState, Node, NodeId, NodeInfo};
use serde::{Deserialize, Serialize};

use crate::layout::Layout;

/// Full throttle maps to 126 "mph", which DCC command stations treat as
/// speed step 126 of 128.
pub const FULL_SPEED_MPH: f32 = 126.0;
const SEARCH_TIMEOUT_MS: u64 = 5_000;
const ASSIGN_TIMEOUT_MS: u64 = 3_000;
const TRAFFIC_LINES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Occupancy {
    #[default]
    Unknown,
    Clear,
    Occupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Position {
    #[default]
    Unknown,
    Normal,
    Reverse,
}

impl Position {
    pub fn toggled(self) -> Position {
        match self {
            Position::Normal => Position::Reverse,
            _ => Position::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnoutStatus {
    /// Last known position (from feedback, or the last command seen).
    pub position: Position,
    /// Commanded position still waiting for feedback.
    pub pending: Option<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// No connection to the bus.
    Offline,
    /// Connected; reserving our alias.
    LoggingIn,
    /// Logged in and exchanging messages.
    Online,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThrottleState {
    Searching,
    Assigning,
    Active,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Throttle {
    pub id: u32,
    pub address: u16,
    pub long_address: bool,
    pub state: ThrottleState,
    pub train_alias: Option<u16>,
    /// 0.0..=1.0 of full speed.
    pub speed: f32,
    pub reverse: bool,
    pub functions: [bool; 29],
    search_event: EventId,
    deadline: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Occupied(String),
    Clear(String),
    FeedbackNormal(String),
    FeedbackReverse(String),
    CommandNormal(String),
    CommandReverse(String),
}

pub struct Dispatcher {
    node: Node,
    layout: Layout,
    index: BTreeMap<EventId, Vec<Action>>,
    pub blocks: BTreeMap<String, Occupancy>,
    pub turnouts: BTreeMap<String, TurnoutStatus>,
    pub throttles: Vec<Throttle>,
    next_throttle: u32,
    link_up: bool,
    synced: bool,
    /// Recent decoded traffic, newest last, for the bus monitor.
    pub traffic: VecDeque<String>,
    /// Recent events reported on the bus (newest first), so a layout editor
    /// can "learn" an event ID by having someone press a button on the layout.
    pub recent_events: VecDeque<EventId>,
    pub monitor: bool,
    /// Incremented whenever anything visible changes; UIs re-render on change.
    pub revision: u64,
}

impl Dispatcher {
    pub fn new(node_id: NodeId, layout: Layout) -> Self {
        let info = NodeInfo {
            manufacturer: "Uncommon Models".into(),
            model: "System4Dispatch".into(),
            hardware_version: "Web".into(),
            software_version: env!("CARGO_PKG_VERSION").into(),
            user_name: "Dispatcher".into(),
            user_description: String::new(),
        };
        let mut d = Dispatcher {
            node: Node::new(node_id, info),
            layout: Layout::empty(""),
            index: BTreeMap::new(),
            blocks: BTreeMap::new(),
            turnouts: BTreeMap::new(),
            throttles: Vec::new(),
            next_throttle: 1,
            link_up: false,
            synced: false,
            traffic: VecDeque::new(),
            monitor: true,
            recent_events: VecDeque::new(),
            revision: 0,
        };
        d.set_layout(layout);
        d
    }

    pub fn node_id(&self) -> NodeId {
        self.node.node_id()
    }

    pub fn alias(&self) -> Option<u16> {
        self.node.alias()
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn duplicate_node_id(&self) -> bool {
        self.node.duplicate_node_id_detected()
    }

    pub fn link_state(&self) -> LinkState {
        match (self.link_up, self.node.is_permitted()) {
            (false, _) => LinkState::Offline,
            (true, false) => LinkState::LoggingIn,
            (true, true) => LinkState::Online,
        }
    }

    /// Replace the layout. Known block/turnout state is kept for ids that
    /// still exist, and the bus is re-queried.
    pub fn set_layout(&mut self, layout: Layout) {
        self.index.clear();
        self.node.clear_events();
        let add = |index: &mut BTreeMap<EventId, Vec<Action>>, e: Option<EventId>, a: Action| {
            if let Some(e) = e {
                index.entry(e).or_default().push(a);
            }
        };
        for b in &layout.blocks {
            add(
                &mut self.index,
                b.occupied_event,
                Action::Occupied(b.id.clone()),
            );
            add(&mut self.index, b.clear_event, Action::Clear(b.id.clone()));
            for e in [b.occupied_event, b.clear_event].into_iter().flatten() {
                self.node.add_consumer(e, EventState::Unknown);
            }
        }
        for t in &layout.turnouts {
            add(
                &mut self.index,
                t.normal_event,
                Action::CommandNormal(t.id.clone()),
            );
            add(
                &mut self.index,
                t.reverse_event,
                Action::CommandReverse(t.id.clone()),
            );
            for e in [t.normal_event, t.reverse_event].into_iter().flatten() {
                self.node.add_producer(e, EventState::Unknown);
            }
            if let Some(fb) = &t.feedback {
                add(
                    &mut self.index,
                    fb.normal_event,
                    Action::FeedbackNormal(t.id.clone()),
                );
                add(
                    &mut self.index,
                    fb.reverse_event,
                    Action::FeedbackReverse(t.id.clone()),
                );
                for e in [fb.normal_event, fb.reverse_event].into_iter().flatten() {
                    self.node.add_consumer(e, EventState::Unknown);
                }
            }
        }
        let old_blocks = std::mem::take(&mut self.blocks);
        let old_turnouts = std::mem::take(&mut self.turnouts);
        for b in &layout.blocks {
            self.blocks.insert(
                b.id.clone(),
                old_blocks.get(&b.id).copied().unwrap_or_default(),
            );
        }
        for t in &layout.turnouts {
            self.turnouts.insert(
                t.id.clone(),
                old_turnouts.get(&t.id).copied().unwrap_or_default(),
            );
        }
        self.layout = layout;
        self.synced = false;
        self.revision += 1;
    }

    /// The link to the bus came up: log in.
    pub fn link_up(&mut self, now: u64) {
        self.revision += 1;
        self.node.reset();
        self.node.start(now);
        self.link_up = true;
        self.synced = false;
        self.log("link up".into());
    }

    /// The link dropped: everything becomes unknown.
    pub fn link_down(&mut self) {
        self.revision += 1;
        self.node.reset();
        self.link_up = false;
        for b in self.blocks.values_mut() {
            *b = Occupancy::Unknown;
        }
        for t in self.turnouts.values_mut() {
            *t = TurnoutStatus::default();
        }
        for t in &mut self.throttles {
            t.state = ThrottleState::Failed("connection lost".into());
            t.train_alias = None;
        }
        self.log("link down".into());
    }

    /// Next time [`Dispatcher::poll`] must run.
    pub fn next_deadline(&self) -> Option<u64> {
        let t = self
            .throttles
            .iter()
            .filter(|t| matches!(t.state, ThrottleState::Searching | ThrottleState::Assigning))
            .map(|t| t.deadline)
            .min();
        match (self.node.next_deadline(), t) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn poll(&mut self, now: u64) {
        if !self.link_up {
            return;
        }
        let was_permitted = self.node.is_permitted();
        self.node.poll(now);
        if self.node.is_permitted() != was_permitted {
            self.revision += 1;
        }
        if self.node.is_permitted() && !self.synced {
            self.synced = true;
            self.query_state();
        }
        for t in &mut self.throttles {
            if now >= t.deadline {
                match t.state {
                    ThrottleState::Searching => {
                        self.revision += 1;
                        t.state = ThrottleState::Failed(format!(
                            "no train found for address {}",
                            t.address
                        ))
                    }
                    ThrottleState::Assigning => {
                        self.revision += 1;
                        t.state = ThrottleState::Failed("train did not respond".into())
                    }
                    _ => {}
                }
            }
        }
    }

    /// Ask producers for the current state of every block and feedback event.
    pub fn query_state(&mut self) {
        let mut events = Vec::new();
        for b in &self.layout.blocks {
            events.extend(b.occupied_event);
            events.extend(b.clear_event);
        }
        for t in &self.layout.turnouts {
            if let Some(fb) = &t.feedback {
                events.extend(fb.normal_event);
                events.extend(fb.reverse_event);
            }
        }
        for e in events {
            self.send(Message::IdentifyProducer(e));
        }
        // Turnout drivers report their current output as Consumer Identified.
        let no_feedback: Vec<EventId> = self
            .layout
            .turnouts
            .iter()
            .filter(|t| !t.has_feedback())
            .flat_map(|t| [t.normal_event, t.reverse_event])
            .flatten()
            .collect();
        for e in no_feedback {
            self.send(Message::IdentifyConsumer(e));
        }
    }

    pub fn take_outgoing(&mut self) -> Vec<CanFrame> {
        self.node.take_outgoing()
    }

    fn send(&mut self, msg: Message) {
        if self.monitor {
            self.log(format!("→ {msg:?}"));
        }
        self.node.send(msg);
    }

    fn log(&mut self, s: String) {
        self.revision += 1;
        if self.traffic.len() >= TRAFFIC_LINES {
            self.traffic.pop_front();
        }
        self.traffic.push_back(s);
    }

    /// Process one frame from the bus. Returns true if visible state changed.
    pub fn receive(&mut self, frame: &CanFrame, now: u64) -> bool {
        let changed = self.receive_inner(frame, now);
        if changed {
            self.revision += 1;
        }
        changed
    }

    fn receive_inner(&mut self, frame: &CanFrame, now: u64) -> bool {
        let Some(rx) = self.node.receive(frame, now) else {
            return false;
        };
        if self.monitor {
            self.log(format!("← {:03X} {:?}", rx.src_alias, rx.msg));
        }
        let me = self.node.alias();
        match rx.msg {
            Message::Pcer(e) => {
                self.remember_event(e);
                self.apply(e, true, false)
            }
            Message::ProducerIdentified { event, state } => {
                let mut changed = self.search_reply(event, rx.src_alias, now);
                match state {
                    EventState::Valid => changed |= self.apply(event, true, false),
                    EventState::Invalid => changed |= self.apply(event, false, false),
                    EventState::Unknown => {}
                }
                changed
            }
            Message::ConsumerIdentified {
                event,
                state: EventState::Valid,
            } => self.apply(event, true, true),
            Message::TractionReply { dest, payload } if Some(dest) == me => {
                self.traction_reply(rx.src_alias, Reply::decode(&payload))
            }
            Message::OptionalInteractionRejected { dest, mti, code } if Some(dest) == me => {
                let mut changed = false;
                if mti == Mti::TRACTION_CONTROL_COMMAND {
                    for t in self
                        .throttles
                        .iter_mut()
                        .filter(|t| t.train_alias == Some(rx.src_alias))
                    {
                        t.state = ThrottleState::Failed(format!(
                            "train rejected command (error {code:04X})"
                        ));
                        changed = true;
                    }
                }
                changed
            }
            _ => false,
        }
    }

    fn remember_event(&mut self, e: EventId) {
        self.recent_events.retain(|x| *x != e);
        self.recent_events.push_front(e);
        self.recent_events.truncate(8);
    }

    /// Apply an event. `happened` = PCER / Identified Valid; false = Identified
    /// Invalid (the event's condition is currently false).
    fn apply(&mut self, e: EventId, happened: bool, consumer_report: bool) -> bool {
        let Some(actions) = self.index.get(&e).cloned() else {
            return false;
        };
        let mut changed = false;
        for a in &actions {
            match a {
                Action::Occupied(id) | Action::Clear(id) if !consumer_report => {
                    let occ = matches!(a, Action::Occupied(_)) == happened;
                    let v = if occ {
                        Occupancy::Occupied
                    } else {
                        Occupancy::Clear
                    };
                    if let Some(b) = self.blocks.get_mut(id) {
                        changed |= *b != v;
                        *b = v;
                    }
                }
                Action::FeedbackNormal(id) | Action::FeedbackReverse(id)
                    if happened && !consumer_report =>
                {
                    let p = if matches!(a, Action::FeedbackNormal(_)) {
                        Position::Normal
                    } else {
                        Position::Reverse
                    };
                    if let Some(t) = self.turnouts.get_mut(id) {
                        changed |= t.position != p || t.pending.is_some();
                        t.position = p;
                        t.pending = None;
                    }
                }
                Action::CommandNormal(id) | Action::CommandReverse(id) if happened => {
                    // Someone (another panel, a fascia button) commanded it.
                    // Without feedback, that is our best knowledge.
                    let p = if matches!(a, Action::CommandNormal(_)) {
                        Position::Normal
                    } else {
                        Position::Reverse
                    };
                    let fb = self.layout.turnout(id).is_some_and(|t| t.has_feedback());
                    if let Some(t) = self.turnouts.get_mut(id) {
                        if fb {
                            if !consumer_report {
                                changed |= t.pending != Some(p);
                                t.pending = Some(p);
                            }
                        } else {
                            changed |= t.position != p;
                            t.position = p;
                        }
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// Command a turnout. Returns false if it can't be sent right now.
    pub fn throw(&mut self, id: &str, pos: Position) -> bool {
        self.revision += 1;
        if self.link_state() != LinkState::Online {
            return false;
        }
        let Some(t) = self.layout.turnout(id).cloned() else {
            return false;
        };
        let event = match pos {
            Position::Normal => t.normal_event,
            Position::Reverse => t.reverse_event,
            Position::Unknown => None,
        };
        let Some(event) = event else { return false };
        self.send(Message::Pcer(event));
        let other = if pos == Position::Normal {
            t.reverse_event
        } else {
            t.normal_event
        };
        self.node.set_producer_state(event, EventState::Valid);
        if let Some(o) = other {
            self.node.set_producer_state(o, EventState::Invalid);
        }
        if let Some(st) = self.turnouts.get_mut(id) {
            if t.has_feedback() {
                st.pending = Some(pos);
            } else {
                st.position = pos;
            }
        }
        true
    }

    /// Throw a turnout to the opposite of its current (or pending) position.
    pub fn toggle(&mut self, id: &str) -> bool {
        let st = self.turnouts.get(id).copied().unwrap_or_default();
        self.throw(id, st.pending.unwrap_or(st.position).toggled())
    }

    // ---- Throttles -------------------------------------------------------

    /// Start acquiring a train by DCC address. Returns the throttle id.
    pub fn acquire(&mut self, address: u16, long_address: bool, now: u64) -> u32 {
        self.revision += 1;
        let id = self.next_throttle;
        self.next_throttle += 1;
        let search_event = search::dcc_event(address, long_address, search::ALLOCATE);
        let online = self.link_state() == LinkState::Online;
        self.throttles.push(Throttle {
            id,
            address,
            long_address,
            state: if online {
                ThrottleState::Searching
            } else {
                ThrottleState::Failed("not connected".into())
            },
            train_alias: None,
            speed: 0.0,
            reverse: false,
            functions: [false; 29],
            search_event,
            deadline: now + SEARCH_TIMEOUT_MS,
        });
        if online {
            self.send(Message::IdentifyProducer(search_event));
        }
        id
    }

    /// Retry a failed throttle.
    pub fn retry(&mut self, id: u32, now: u64) {
        if self.link_state() != LinkState::Online {
            return;
        }
        let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) else {
            return;
        };
        t.state = ThrottleState::Searching;
        t.train_alias = None;
        t.deadline = now + SEARCH_TIMEOUT_MS;
        let e = t.search_event;
        self.send(Message::IdentifyProducer(e));
    }

    fn search_reply(&mut self, event: EventId, src: u16, now: u64) -> bool {
        if !search::is_search_event(event) {
            return false;
        }
        let me = self.node.node_id();
        let mut cmds = Vec::new();
        for t in &mut self.throttles {
            if t.search_event == event && t.state == ThrottleState::Searching {
                t.train_alias = Some(src);
                t.state = ThrottleState::Assigning;
                t.deadline = now + ASSIGN_TIMEOUT_MS;
                cmds.push((
                    src,
                    Command::AssignController {
                        flags: 0,
                        controller: me,
                    },
                ));
            }
        }
        let changed = !cmds.is_empty();
        for (dest, c) in cmds {
            self.traction(dest, c);
        }
        changed
    }

    fn traction(&mut self, dest: u16, c: Command) {
        self.send(Message::TractionCommand {
            dest,
            payload: c.encode(),
        });
    }

    fn traction_reply(&mut self, src: u16, reply: Reply) -> bool {
        let mut changed = false;
        let mut cmds = Vec::new();
        match reply {
            Reply::AssignController { result } => {
                for t in self
                    .throttles
                    .iter_mut()
                    .filter(|t| t.train_alias == Some(src))
                {
                    if t.state != ThrottleState::Assigning {
                        continue;
                    }
                    changed = true;
                    if result == 0 {
                        t.state = ThrottleState::Active;
                        cmds.push(Command::QuerySpeed);
                        for f in 0..=4 {
                            cmds.push(Command::QueryFunction { addr: f });
                        }
                    } else {
                        t.state =
                            ThrottleState::Failed("train is controlled by another throttle".into());
                    }
                }
            }
            Reply::QuerySpeed { set_speed, .. } => {
                for t in self
                    .throttles
                    .iter_mut()
                    .filter(|t| t.train_alias == Some(src))
                {
                    if !set_speed.is_nan() {
                        t.speed = (set_speed.mph() / FULL_SPEED_MPH).clamp(0.0, 1.0);
                        t.reverse = set_speed.is_reverse();
                        changed = true;
                    }
                }
            }
            Reply::QueryFunction { addr, value } => {
                for t in self
                    .throttles
                    .iter_mut()
                    .filter(|t| t.train_alias == Some(src))
                {
                    if let Some(f) = t.functions.get_mut(addr as usize) {
                        *f = value != 0;
                        changed = true;
                    }
                }
            }
            Reply::Heartbeat { .. } => {
                if self
                    .throttles
                    .iter()
                    .any(|t| t.train_alias == Some(src) && t.state == ThrottleState::Active)
                {
                    cmds.push(Command::Noop);
                }
            }
            Reply::ControllerChanged { .. } => {}
            _ => {}
        }
        for c in cmds {
            self.traction(src, c);
        }
        changed
    }

    fn active_alias(&self, id: u32) -> Option<u16> {
        self.throttles
            .iter()
            .find(|t| t.id == id && t.state == ThrottleState::Active)
            .and_then(|t| t.train_alias)
    }

    pub fn set_speed(&mut self, id: u32, speed: f32, reverse: bool) {
        self.revision += 1;
        let speed = speed.clamp(0.0, 1.0);
        if let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) {
            t.speed = speed;
            t.reverse = reverse;
        }
        if let Some(a) = self.active_alias(id) {
            self.traction(
                a,
                Command::SetSpeed(Speed::from_mph(speed * FULL_SPEED_MPH, reverse)),
            );
        }
    }

    pub fn set_function(&mut self, id: u32, f: u8, on: bool) {
        self.revision += 1;
        if let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) {
            if let Some(slot) = t.functions.get_mut(f as usize) {
                *slot = on;
            }
        }
        if let Some(a) = self.active_alias(id) {
            self.traction(
                a,
                Command::SetFunction {
                    addr: f as u32,
                    value: on as u16,
                },
            );
        }
    }

    pub fn emergency_stop(&mut self, id: u32) {
        self.revision += 1;
        if let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) {
            t.speed = 0.0;
        }
        if let Some(a) = self.active_alias(id) {
            self.traction(a, Command::EmergencyStop);
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<u32> = self.throttles.iter().map(|t| t.id).collect();
        for id in ids {
            self.emergency_stop(id);
        }
    }

    /// Release the train and close the throttle.
    pub fn release(&mut self, id: u32) {
        self.revision += 1;
        if let Some(a) = self.active_alias(id) {
            let me = self.node.node_id();
            self.traction(
                a,
                Command::ReleaseController {
                    flags: 0,
                    controller: me,
                },
            );
        }
        self.throttles.retain(|t| t.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Simulator;

    /// Run dispatcher and simulator against each other on a virtual clock.
    struct Bench {
        d: Dispatcher,
        s: Simulator,
        now: u64,
    }

    impl Bench {
        fn new() -> Self {
            let layout = Layout::demo();
            let mut s = Simulator::new(layout.clone(), 0);
            s.random_occupancy = false;
            let mut d = Dispatcher::new(NodeId::new(0x0501_0101_2299), layout);
            d.link_up(0);
            Bench { d, s, now: 0 }
        }

        fn run(&mut self, ms: u64) {
            let end = self.now + ms;
            while self.now < end {
                self.now += 10;
                self.d.poll(self.now);
                self.s.poll(self.now);
                for _ in 0..10 {
                    let a = self.d.take_outgoing();
                    let b = self.s.take_outgoing();
                    if a.is_empty() && b.is_empty() {
                        break;
                    }
                    for f in &a {
                        self.s.receive(f, self.now);
                    }
                    for f in &b {
                        self.d.receive(f, self.now);
                    }
                }
            }
        }
    }

    #[test]
    fn logs_in_and_learns_block_state() {
        let mut b = Bench::new();
        b.s.set_occupied("B2", true);
        assert_eq!(b.d.link_state(), LinkState::LoggingIn);
        b.run(500);
        assert_eq!(b.d.link_state(), LinkState::Online);
        assert_eq!(b.d.blocks["B2"], Occupancy::Occupied);
        assert_eq!(b.d.blocks["B1"], Occupancy::Clear);
        assert_eq!(b.d.turnouts["T1"].position, Position::Normal);

        b.s.set_occupied("B1", true);
        b.run(50);
        assert_eq!(b.d.blocks["B1"], Occupancy::Occupied);
    }

    #[test]
    fn throw_with_and_without_feedback() {
        let mut b = Bench::new();
        b.run(500);
        assert!(b.d.throw("T1", Position::Reverse));
        assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
        b.run(1000);
        assert_eq!(
            b.d.turnouts["T1"],
            TurnoutStatus {
                position: Position::Reverse,
                pending: None
            }
        );
        assert_eq!(b.s.turnout_position("T1"), Some(Position::Reverse));

        // T3 has no feedback: optimistic.
        assert!(b.d.toggle("T3"));
        assert_eq!(b.d.turnouts["T3"].position, Position::Reverse);
        b.run(50);
        assert_eq!(b.s.turnout_position("T3"), Some(Position::Reverse));
    }

    #[test]
    fn throttle_acquire_drive_release() {
        let mut b = Bench::new();
        b.run(500);
        let id = b.d.acquire(1234, false, b.now);
        assert_eq!(b.d.throttles[0].state, ThrottleState::Searching);
        b.run(600);
        assert_eq!(b.d.throttles[0].state, ThrottleState::Active);

        b.d.set_speed(id, 0.5, true);
        b.d.set_function(id, 0, true);
        b.run(50);
        let train = b.s.train(1234).unwrap();
        assert!(train.speed.is_reverse());
        assert!((train.speed.mph() - 63.0).abs() < 0.5);
        assert!(train.functions[0]);
        assert!(train.controller.is_some());

        b.d.release(id);
        b.run(50);
        assert!(b.d.throttles.is_empty());
        assert!(b.s.train(1234).unwrap().controller.is_none());
    }

    #[test]
    fn heartbeat_is_answered() {
        let mut b = Bench::new();
        b.run(500);
        b.d.acquire(3, false, b.now);
        b.run(600);
        let before = b.s.train(3).unwrap().noops;
        b.run(crate::sim::HEARTBEAT_MS + 100);
        assert!(b.s.train(3).unwrap().noops > before);
    }

    #[test]
    fn link_down_resets() {
        let mut b = Bench::new();
        b.run(500);
        b.d.acquire(3, false, b.now);
        b.run(600);
        b.d.link_down();
        assert_eq!(b.d.link_state(), LinkState::Offline);
        assert_eq!(b.d.blocks["B1"], Occupancy::Unknown);
        assert!(matches!(b.d.throttles[0].state, ThrottleState::Failed(_)));
        assert!(!b.d.throw("T1", Position::Reverse));
    }

    #[test]
    fn search_times_out() {
        let mut d = Dispatcher::new(NodeId::new(0x0501_0101_2299), Layout::demo());
        d.link_up(0);
        d.poll(300);
        d.acquire(99, false, 300);
        d.poll(300 + SEARCH_TIMEOUT_MS);
        assert!(matches!(d.throttles[0].state, ThrottleState::Failed(_)));
    }
}
