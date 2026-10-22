//! The dispatcher: one OpenLCB node that tracks block occupancy and turnout
//! positions for a [`Layout`], throws turnouts, and runs throttles.
//!
//! Like `rolcb::Node` it is sans-IO: feed it frames with
//! [`Dispatcher::receive`] and time with [`Dispatcher::poll`], and collect
//! frames to send with [`Dispatcher::take_outgoing`].
//!
//! # Time
//!
//! Every `now` argument is a timestamp in **milliseconds** from an arbitrary
//! epoch (`performance.now()`, an `Instant` offset, or a virtual clock in
//! tests). All calls on one dispatcher must use the same clock, and it must
//! never go backwards. Call [`Dispatcher::poll`] at or after
//! [`Dispatcher::next_deadline`]; calling it more often (say every 50 ms) is
//! harmless. Methods without a `now` argument, such as
//! [`Dispatcher::throw`], use the time of the last call that had one.
//!
//! # Change detection: `revision`
//!
//! [`Dispatcher::revision`] increases whenever anything a UI might show
//! changes (block or turnout state, throttles, link state, traffic log). It
//! never decreases and may also increase when nothing visible changed, so a
//! UI should keep the last value it rendered and redraw when it differs.
//!
//! # How events update state
//!
//! The layout gives every event a role. Incoming event messages update state
//! as follows (— means ignored):
//!
//! | Event role | PCER | Producer Identified Valid | Producer Identified Invalid | Producer Identified Unknown | Consumer Identified Valid |
//! |---|---|---|---|---|---|
//! | Block `occupied_event` | Occupied | Occupied | Clear | — | — |
//! | Block `clear_event` | Clear | Clear | Occupied | — | — |
//! | Turnout feedback (`normal`/`reverse`) | position := that; clears `stalled`, and `pending` if it matches | same as PCER | — | — | — |
//! | Turnout command, no feedback | position := that | position := that | — | — | position := that |
//! | Turnout command, with feedback | `pending` := that (someone else threw it) | — | — | — | — |
//!
//! Producer Identified on a *command* event is state, not an action: rolcb
//! announces every producer at login, so another panel or JMRI joining the
//! bus must not make turnouts look as if they were being thrown.
//!
//! Whenever a turnout with feedback becomes `pending` (thrown by us or by
//! someone else), the dispatcher sends Identify Producer for the target
//! position's feedback event, because real sensors only report on change:
//! if the points were already there, the Valid reply confirms it. If no
//! confirmation arrives within 5 s, `pending` is dropped and
//! [`TurnoutStatus::stalled`] is set until the next real feedback.
//!
//! The dispatcher's own consumer states for block events follow the known
//! occupancy, so other nodes asking Identify Consumer get a truthful answer.
//!
//! # Throttles
//!
//! ```text
//!   acquire()/retry()
//!          │
//!          ▼
//!     Searching ── no train answers within 5 s ─────────────────┐
//!          │ Producer Identified for the search event           │
//!          ▼                                                    ▼
//!     Assigning ── refused, or no answer within 3 s ──────► Failed(reason)
//!          │ AssignController { result: 0 }                     ▲   │
//!          ▼                                                    │   │ retry()
//!       Active ── rejected command, taken over, link lost ──────┘   ▼
//!                                                               Searching
//!
//!   release() closes a throttle in any state; it sends ReleaseController
//!   if the throttle knows the train's alias and no other throttle shares it.
//! ```
//!
//! [`Dispatcher::acquire`] returns the existing throttle if one that hasn't
//! failed already drives the same address.
//!
//! # Speed scale
//!
//! The Traction protocol carries speeds as physical velocities, but DCC
//! command stations translate them back into speed steps, treating the
//! value in mph as the step number in 128-step mode. Mapping full throttle
//! to [`FULL_SPEED_MPH`] = 126 mph therefore gives one DCC step per slider
//! notch over the whole range, with no dead zone at the top.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use rolcb::message::Message;
use rolcb::mti::Mti;
use rolcb::traction::{search, Command, Reply, Speed};
use rolcb::{CanFrame, EventId, EventState, Node, NodeId, NodeInfo};
use serde::{Deserialize, Serialize};

use crate::layout::Layout;

/// Full throttle maps to 126 "mph", which DCC command stations treat as
/// speed step 126 of 128. See [the module docs](self#speed-scale).
pub const FULL_SPEED_MPH: f32 = 126.0;
const SEARCH_TIMEOUT_MS: u64 = 5_000;
const ASSIGN_TIMEOUT_MS: u64 = 3_000;
/// How long a thrown turnout with feedback may stay pending.
pub const PENDING_TIMEOUT_MS: u64 = 5_000;
const TRAFFIC_LINES: usize = 300;

/// What is known about a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Occupancy {
    /// Nothing heard yet (or the link is down).
    #[default]
    Unknown,
    /// The detector reports the block clear.
    Clear,
    /// The detector reports the block occupied.
    Occupied,
}

/// A turnout position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Position {
    /// Not known.
    #[default]
    Unknown,
    /// Straight through (the turnout's `normal_event`).
    Normal,
    /// Diverging (the turnout's `reverse_event`).
    Reverse,
}

impl Position {
    /// The opposite position; `Unknown` toggles to `Normal`.
    pub fn toggled(self) -> Position {
        match self {
            Position::Normal => Position::Reverse,
            _ => Position::Normal,
        }
    }
}

/// What is known about a turnout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnoutStatus {
    /// Last known position (from feedback, or the last command seen).
    pub position: Position,
    /// Commanded position still waiting for feedback.
    pub pending: Option<Position>,
    /// The last command was not confirmed by feedback in time. Cleared by
    /// the next feedback report.
    pub stalled: bool,
}

/// The state of the dispatcher's connection to the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// No connection to the bus.
    Offline,
    /// Connected; reserving our alias.
    LoggingIn,
    /// Logged in and exchanging messages.
    Online,
}

/// Why a throttle failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// Acquire was attempted while offline.
    NotConnected,
    /// No command station answered the search for this address.
    NoTrain {
        /// The DCC address searched for.
        address: u16,
    },
    /// The train did not answer the controller assignment.
    NoResponse,
    /// The train refused the assignment (another throttle controls it).
    Refused,
    /// The train rejected a command with this OpenLCB error code.
    Rejected(u16),
    /// The link to the bus dropped (or was re-established).
    ConnectionLost,
    /// Another throttle took the train over.
    TakenOver,
}

impl fmt::Display for FailReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailReason::NotConnected => f.write_str("not connected"),
            FailReason::NoTrain { address } => write!(f, "no train found for address {address}"),
            FailReason::NoResponse => f.write_str("train did not respond"),
            FailReason::Refused => f.write_str("train is controlled by another throttle"),
            FailReason::Rejected(code) => write!(f, "train rejected command (error {code:04X})"),
            FailReason::ConnectionLost => f.write_str("connection lost"),
            FailReason::TakenOver => f.write_str("taken over by another throttle"),
        }
    }
}

/// Where a throttle is in acquiring its train (see [the diagram](self#throttles)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThrottleState {
    /// Waiting for a command station to answer the train search.
    Searching,
    /// Found the train; waiting for it to accept us as controller.
    Assigning,
    /// In control.
    Active,
    /// Gave up; [`Dispatcher::retry`] starts again.
    Failed(FailReason),
}

/// A throttle driving one train.
#[derive(Debug, Clone, PartialEq)]
pub struct Throttle {
    /// Identifier returned by [`Dispatcher::acquire`].
    pub id: u32,
    /// DCC address.
    pub address: u16,
    /// DCC long address (always true above 127).
    pub long_address: bool,
    /// Acquisition state.
    pub state: ThrottleState,
    /// The train node's CAN alias, once found.
    pub train_alias: Option<u16>,
    /// 0.0..=1.0 of full speed.
    pub speed: f32,
    /// Direction.
    pub reverse: bool,
    /// Function states F0..=F28 as last set or reported.
    pub functions: [bool; 29],
    search_event: EventId,
    deadline: u64,
}

impl Throttle {
    fn failed(&self) -> bool {
        matches!(self.state, ThrottleState::Failed(_))
    }
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

/// How an event was reported on the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Report {
    Pcer,
    ProducerValid,
    ProducerInvalid,
    ConsumerValid,
}

/// The dispatcher node. See [the module docs](self).
pub struct Dispatcher {
    node: Node,
    layout: Layout,
    index: BTreeMap<EventId, Vec<Action>>,
    /// Occupancy of every block in the layout, by block id.
    pub blocks: BTreeMap<String, Occupancy>,
    /// Status of every turnout in the layout, by turnout id.
    pub turnouts: BTreeMap<String, TurnoutStatus>,
    /// Open throttles, oldest first.
    pub throttles: Vec<Throttle>,
    next_throttle: u32,
    link_up: bool,
    synced: bool,
    now: u64,
    pending_deadlines: BTreeMap<String, u64>,
    /// Recent decoded traffic, newest last, for the bus monitor.
    pub traffic: VecDeque<String>,
    /// Recent events reported on the bus (newest first), so a layout editor
    /// can "learn" an event ID by having someone press a button on the layout.
    pub recent_events: VecDeque<EventId>,
    /// Log every message to [`traffic`](Dispatcher::traffic).
    pub monitor: bool,
    /// Incremented whenever anything visible changes; UIs re-render on change.
    /// See [the module docs](self#change-detection-revision).
    pub revision: u64,
}

impl Dispatcher {
    /// The SNIP identity [`Dispatcher::new`] uses. Its `hardware_version` is
    /// empty; set it (e.g. to `"Web"` or `"Desktop"`) and pass the result to
    /// [`Dispatcher::with_info`].
    pub fn default_info() -> NodeInfo {
        NodeInfo {
            manufacturer: "Uncommon Models".into(),
            model: "System4Dispatch".into(),
            hardware_version: String::new(),
            software_version: env!("CARGO_PKG_VERSION").into(),
            user_name: "Dispatcher".into(),
            user_description: String::new(),
        }
    }

    /// A dispatcher with the [default identity](Dispatcher::default_info).
    /// It stays offline until [`Dispatcher::link_up`].
    pub fn new(node_id: NodeId, layout: Layout) -> Self {
        Self::with_info(node_id, layout, Self::default_info())
    }

    /// A dispatcher that reports `info` in SNIP replies.
    pub fn with_info(node_id: NodeId, layout: Layout, info: NodeInfo) -> Self {
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
            now: 0,
            pending_deadlines: BTreeMap::new(),
            traffic: VecDeque::new(),
            monitor: true,
            recent_events: VecDeque::new(),
            revision: 0,
        };
        d.set_layout(layout);
        d
    }

    /// Our Node ID.
    pub fn node_id(&self) -> NodeId {
        self.node.node_id()
    }

    /// Our CAN alias, once logged in.
    pub fn alias(&self) -> Option<u16> {
        self.node.alias()
    }

    /// The layout in use.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Another node on the bus uses our Node ID.
    pub fn duplicate_node_id(&self) -> bool {
        self.node.duplicate_node_id_detected()
    }

    /// The state of the link.
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
        self.pending_deadlines
            .retain(|id, _| layout.turnout(id).is_some_and(|t| t.has_feedback()));
        for (id, st) in &mut self.turnouts {
            if st.pending.is_some() && !self.pending_deadlines.contains_key(id) {
                st.pending = None;
            }
        }
        self.layout = layout;
        self.sync_consumer_states();
        self.synced = false;
        self.revision += 1;
    }

    /// The link to the bus came up: log in. Throttles that were still
    /// running are failed with [`FailReason::ConnectionLost`], since their
    /// trains may no longer be ours.
    pub fn link_up(&mut self, now: u64) {
        self.revision += 1;
        self.now = now;
        self.node.reset();
        self.node.start(now);
        self.link_up = true;
        self.synced = false;
        self.fail_running_throttles(FailReason::ConnectionLost);
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
        self.pending_deadlines.clear();
        self.sync_consumer_states();
        self.fail_running_throttles(FailReason::ConnectionLost);
        self.log("link down".into());
    }

    fn fail_running_throttles(&mut self, why: FailReason) {
        for t in &mut self.throttles {
            if !t.failed() {
                t.state = ThrottleState::Failed(why);
                t.train_alias = None;
            }
        }
    }

    /// Next time [`Dispatcher::poll`] must run.
    pub fn next_deadline(&self) -> Option<u64> {
        let throttles = self
            .throttles
            .iter()
            .filter(|t| matches!(t.state, ThrottleState::Searching | ThrottleState::Assigning))
            .map(|t| t.deadline);
        let pending = if self.link_up {
            self.pending_deadlines.values().copied().min()
        } else {
            None
        };
        throttles
            .chain(pending)
            .chain(self.node.next_deadline())
            .min()
    }

    /// Run timers: login, initial state query, throttle and pending timeouts.
    pub fn poll(&mut self, now: u64) {
        self.now = now;
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
                        t.state = ThrottleState::Failed(FailReason::NoTrain { address: t.address })
                    }
                    ThrottleState::Assigning => {
                        self.revision += 1;
                        t.state = ThrottleState::Failed(FailReason::NoResponse)
                    }
                    _ => {}
                }
            }
        }
        let expired: Vec<String> = self
            .pending_deadlines
            .iter()
            .filter(|(_, &d)| now >= d)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.pending_deadlines.remove(&id);
            if let Some(st) = self.turnouts.get_mut(&id) {
                if st.pending.take().is_some() {
                    st.stalled = true;
                    self.revision += 1;
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

    /// Drain frames to put on the bus.
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
        self.now = now;
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
                self.apply(e, Report::Pcer)
            }
            Message::ProducerIdentified { event, state } => {
                let mut changed = self.search_reply(event, rx.src_alias, now);
                match state {
                    EventState::Valid => changed |= self.apply(event, Report::ProducerValid),
                    EventState::Invalid => changed |= self.apply(event, Report::ProducerInvalid),
                    EventState::Unknown => {}
                }
                changed
            }
            Message::ConsumerIdentified {
                event,
                state: EventState::Valid,
            } => self.apply(event, Report::ConsumerValid),
            Message::TractionReply { dest, payload } if Some(dest) == me => {
                self.traction_reply(rx.src_alias, Reply::decode(&payload))
            }
            // A train tells its controller (us) that another throttle is
            // taking over: acknowledge, and stop driving it.
            Message::TractionCommand { dest, payload } if Some(dest) == me => {
                match Command::decode(&payload) {
                    Command::ControllerChangingNotify { .. } => {
                        self.traction_reply_to(
                            rx.src_alias,
                            Reply::ControllerChanged { result: 0 },
                        );
                        self.lose_train(rx.src_alias)
                    }
                    _ => false,
                }
            }
            Message::OptionalInteractionRejected { dest, mti, code } if Some(dest) == me => {
                let mut changed = false;
                if mti == Mti::TRACTION_CONTROL_COMMAND {
                    for t in self
                        .throttles
                        .iter_mut()
                        .filter(|t| t.train_alias == Some(rx.src_alias) && !t.failed())
                    {
                        t.state = ThrottleState::Failed(FailReason::Rejected(code));
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

    /// Apply a reported event according to the table in the module docs.
    fn apply(&mut self, e: EventId, report: Report) -> bool {
        let Some(actions) = self.index.get(&e).cloned() else {
            return false;
        };
        let mut changed = false;
        let mut blocks_changed = false;
        for a in &actions {
            match a {
                Action::Occupied(id) | Action::Clear(id) => {
                    let holds = match report {
                        Report::Pcer | Report::ProducerValid => true,
                        Report::ProducerInvalid => false,
                        Report::ConsumerValid => continue,
                    };
                    let v = if matches!(a, Action::Occupied(_)) == holds {
                        Occupancy::Occupied
                    } else {
                        Occupancy::Clear
                    };
                    if let Some(b) = self.blocks.get_mut(id) {
                        if *b != v {
                            *b = v;
                            changed = true;
                            blocks_changed = true;
                        }
                    }
                }
                Action::FeedbackNormal(id) | Action::FeedbackReverse(id) => {
                    if !matches!(report, Report::Pcer | Report::ProducerValid) {
                        continue;
                    }
                    let p = if matches!(a, Action::FeedbackNormal(_)) {
                        Position::Normal
                    } else {
                        Position::Reverse
                    };
                    if let Some(t) = self.turnouts.get_mut(id) {
                        let before = *t;
                        t.position = p;
                        t.stalled = false;
                        if t.pending == Some(p) {
                            t.pending = None;
                            self.pending_deadlines.remove(id);
                        }
                        changed |= *t != before;
                    }
                }
                Action::CommandNormal(id) | Action::CommandReverse(id) => {
                    let p = if matches!(a, Action::CommandNormal(_)) {
                        Position::Normal
                    } else {
                        Position::Reverse
                    };
                    let fb = self.layout.turnout(id).is_some_and(|t| t.has_feedback());
                    match (report, fb) {
                        // Someone else (another panel, a fascia button)
                        // threw it: wait for feedback.
                        (Report::Pcer, true) => changed |= self.set_pending(id, p),
                        // Without feedback, the last command is our best
                        // knowledge of the position.
                        (Report::Pcer | Report::ProducerValid | Report::ConsumerValid, false) => {
                            if let Some(t) = self.turnouts.get_mut(id) {
                                changed |= t.position != p;
                                t.position = p;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if blocks_changed {
            self.sync_consumer_states();
        }
        changed
    }

    /// Mark a turnout with feedback as moving to `pos`, arm the deadline and
    /// ask its sensor whether it is already there.
    fn set_pending(&mut self, id: &str, pos: Position) -> bool {
        let Some(st) = self.turnouts.get_mut(id) else {
            return false;
        };
        let changed = st.pending != Some(pos);
        st.pending = Some(pos);
        self.pending_deadlines
            .insert(id.to_string(), self.now + PENDING_TIMEOUT_MS);
        let target = self
            .layout
            .turnout(id)
            .and_then(|t| t.feedback.as_ref())
            .and_then(|fb| match pos {
                Position::Normal => fb.normal_event,
                Position::Reverse => fb.reverse_event,
                Position::Unknown => None,
            });
        if let Some(e) = target {
            self.send(Message::IdentifyProducer(e));
        }
        changed
    }

    /// Make our consumer states match what we know, so that Identify
    /// Consumer gets a truthful answer.
    fn sync_consumer_states(&mut self) {
        let state = |known: Option<bool>| match known {
            Some(true) => EventState::Valid,
            Some(false) => EventState::Invalid,
            None => EventState::Unknown,
        };
        for b in &self.layout.blocks {
            let occ = match self.blocks.get(&b.id).copied().unwrap_or_default() {
                Occupancy::Occupied => Some(true),
                Occupancy::Clear => Some(false),
                Occupancy::Unknown => None,
            };
            if let Some(e) = b.occupied_event {
                self.node.set_consumer_state(e, state(occ));
            }
            if let Some(e) = b.clear_event {
                self.node.set_consumer_state(e, state(occ.map(|o| !o)));
            }
        }
    }

    /// Command a turnout, and the turnout linked to it if there is one.
    /// Returns false if it can't be sent right now.
    pub fn throw(&mut self, id: &str, pos: Position) -> bool {
        self.revision += 1;
        if self.link_state() != LinkState::Online || !self.throw_one(id, pos) {
            return false;
        }
        if let Some((partner, inverted)) = self.layout.linked_partner(id) {
            let p = if inverted { pos.toggled() } else { pos };
            self.throw_one(&partner, p);
        }
        true
    }

    fn throw_one(&mut self, id: &str, pos: Position) -> bool {
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
        if t.has_feedback() {
            self.set_pending(id, pos);
        } else if let Some(st) = self.turnouts.get_mut(id) {
            st.position = pos;
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
    ///
    /// If a throttle that hasn't failed already drives this train (same
    /// address and address kind; addresses above 127 are always long), its
    /// id is returned instead of opening a second one.
    pub fn acquire(&mut self, address: u16, long_address: bool, now: u64) -> u32 {
        self.revision += 1;
        self.now = now;
        let long_address = long_address || address > 127;
        if let Some(t) = self
            .throttles
            .iter()
            .find(|t| t.address == address && t.long_address == long_address && !t.failed())
        {
            return t.id;
        }
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
                ThrottleState::Failed(FailReason::NotConnected)
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
        self.now = now;
        if self.link_state() != LinkState::Online {
            return;
        }
        let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) else {
            return;
        };
        self.revision += 1;
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

    fn release_command(&self) -> Command {
        Command::ReleaseController {
            flags: 0,
            controller: self.node.node_id(),
        }
    }

    /// Mark every live throttle on the train at `alias` as taken over.
    fn lose_train(&mut self, alias: u16) -> bool {
        let mut changed = false;
        for t in self
            .throttles
            .iter_mut()
            .filter(|t| t.train_alias == Some(alias) && !t.failed())
        {
            t.state = ThrottleState::Failed(FailReason::TakenOver);
            t.train_alias = None;
            changed = true;
        }
        changed
    }

    fn traction_reply_to(&mut self, dest: u16, r: Reply) {
        self.send(Message::TractionReply {
            dest,
            payload: r.encode(),
        });
    }

    fn traction_reply(&mut self, src: u16, reply: Reply) -> bool {
        let mut changed = false;
        let mut cmds = Vec::new();
        match reply {
            Reply::AssignController { result } => {
                let mut matched = false;
                for t in self
                    .throttles
                    .iter_mut()
                    .filter(|t| t.train_alias == Some(src))
                {
                    match t.state {
                        ThrottleState::Assigning => {
                            matched = true;
                            changed = true;
                            if result == 0 {
                                t.state = ThrottleState::Active;
                                cmds.push(Command::QuerySpeed);
                                for f in 0..=4 {
                                    cmds.push(Command::QueryFunction { addr: f });
                                }
                            } else {
                                t.state = ThrottleState::Failed(FailReason::Refused);
                            }
                        }
                        ThrottleState::Active => matched = true,
                        _ => {}
                    }
                }
                if !matched && result == 0 {
                    // The train accepted an assignment nobody is waiting for
                    // any more (we released or gave up meanwhile): let go.
                    cmds.push(self.release_command());
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
            Reply::ControllerChanged { .. } => changed |= self.lose_train(src),
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

    /// Set a throttle's speed (0.0..=1.0 of full speed) and direction.
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

    /// Switch a function (0..=28) on or off.
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

    /// Emergency-stop one train.
    pub fn emergency_stop(&mut self, id: u32) {
        self.revision += 1;
        if let Some(t) = self.throttles.iter_mut().find(|t| t.id == id) {
            t.speed = 0.0;
        }
        if let Some(a) = self.active_alias(id) {
            self.traction(a, Command::EmergencyStop);
        }
    }

    /// Emergency-stop every train we control.
    pub fn stop_all(&mut self) {
        let ids: Vec<u32> = self.throttles.iter().map(|t| t.id).collect();
        for id in ids {
            self.emergency_stop(id);
        }
    }

    /// Release the train and close the throttle.
    ///
    /// ReleaseController is sent whenever the throttle knows the train's
    /// alias (also while still assigning, or after the assignment timed
    /// out), unless another live throttle shares that train.
    pub fn release(&mut self, id: u32) {
        self.revision += 1;
        let alias = self
            .throttles
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.train_alias);
        if let Some(a) = alias {
            let shared = self
                .throttles
                .iter()
                .any(|o| o.id != id && o.train_alias == Some(a) && !o.failed());
            if !shared {
                let c = self.release_command();
                self.traction(a, c);
            }
        }
        self.throttles.retain(|t| t.id != id);
    }
}

#[cfg(test)]
mod tests;
