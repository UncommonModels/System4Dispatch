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
//! | Turnout command, with feedback | `pending` := that (someone else threw it) | position := that, if unknown | — | — | position := that, if unknown |
//! | Turntable track `feedback_event` | position := that track; clears `stalled`, and `pending` if it matches | same as PCER | — | — | — |
//! | Turntable `cw_event` / `ccw_event` | the bridge moves one track on from where it was going (someone else turned it) | — | — | — | — |
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
//! Turntables are driven like a turnout with two events: each `cw_event`
//! moves the bridge one track clockwise and each `ccw_event` one track
//! counter-clockwise, so the dispatcher counts steps to know where it is.
//! [`Dispatcher::turn_to`] sends the shortest run of steps to a track.
//! Arriving at a track with a `feedback_event` is waited for (up to
//! [`TURNTABLE_TIMEOUT_MS`], since bridges are slow); a track without
//! feedback is assumed reached. Until a feedback event or
//! [`Dispatcher::set_turntable_position`] says where the bridge is, its
//! position is unknown and only single steps can be sent.
//!
//! # Learning the state on connect
//!
//! Once logged in, [`Dispatcher::query_state`] asks the bus about
//! everything in the layout: Identify Producer for block, feedback and
//! turnout command events (detectors and sensors answer, and so do other
//! panels or JMRI for commands they produce), and Identify Consumer for
//! turnout command events (turnout drivers answer with their output).
//! Nodes that are still starting up miss the first round, so anything
//! still unknown is asked about again [`REQUERY_MS`] later, twice.
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

use rolcb::message::{protocol, Message};
use rolcb::mti::Mti;
use rolcb::traction::{search, Command, Reply, Speed};
use rolcb::{CanFrame, EventId, EventState, Node, NodeId, NodeInfo};
use serde::{Deserialize, Serialize};

use crate::config::{plan_reads, NodeConfig};
use crate::layout::Layout;

/// Full throttle maps to 126 "mph", which DCC command stations treat as
/// speed step 126 of 128. See [the module docs](self#speed-scale).
pub const FULL_SPEED_MPH: f32 = 126.0;
const SEARCH_TIMEOUT_MS: u64 = 5_000;
const ASSIGN_TIMEOUT_MS: u64 = 3_000;
/// How long a thrown turnout with feedback may stay pending.
pub const PENDING_TIMEOUT_MS: u64 = 5_000;
/// How long a turntable may take to reach a position with feedback.
pub const TURNTABLE_TIMEOUT_MS: u64 = 90_000;
const TRAFFIC_LINES: usize = 300;
/// After the state query at login, ask again about anything still unknown
/// this long afterwards (nodes may still be starting up)...
pub const REQUERY_MS: u64 = 3_000;
/// ...up to this many times.
const REQUERY_TRIES: u8 = 2;

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

/// What is known about a turntable. Positions are indexes into its
/// [`tracks`](crate::layout::Turntable::tracks): the track the bridge's head
/// end lines up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurntableStatus {
    /// Last known position (from feedback, counted steps, or set by hand).
    pub position: Option<usize>,
    /// Track the bridge is turning to, still waiting for feedback.
    pub pending: Option<usize>,
    /// The last command was not confirmed by feedback in time. Cleared by
    /// the next feedback report.
    pub stalled: bool,
}

/// How many tracks turntable `id` has in `layout`, if it exists.
fn old_layout_tracks(layout: &Layout, id: &str) -> Option<usize> {
    layout.turntable(id).map(|t| t.tracks.len())
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
    TurntableStep(String, bool),
    TurntableFeedback(String, usize),
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
    /// Status of every turntable in the layout, by turntable id.
    pub turntables: BTreeMap<String, TurntableStatus>,
    /// Open throttles, oldest first.
    pub throttles: Vec<Throttle>,
    next_throttle: u32,
    link_up: bool,
    synced: bool,
    /// When to ask again about blocks and turnouts that are still unknown,
    /// and how many more times to try.
    requery: Option<(u64, u8)>,
    now: u64,
    pending_deadlines: BTreeMap<String, u64>,
    /// Recent decoded traffic, newest last, for the bus monitor.
    pub traffic: VecDeque<String>,
    /// Recent events reported on the bus (newest first), so a layout editor
    /// can "learn" an event ID by having someone press a button on the layout.
    pub recent_events: VecDeque<EventId>,
    /// Log every message to [`traffic`](Dispatcher::traffic).
    pub monitor: bool,
    /// Other nodes on the bus and their configuration; see [`crate::config`].
    pub config: NodeConfig,
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
            turntables: BTreeMap::new(),
            throttles: Vec::new(),
            next_throttle: 1,
            link_up: false,
            synced: false,
            requery: None,
            now: 0,
            pending_deadlines: BTreeMap::new(),
            traffic: VecDeque::new(),
            monitor: true,
            config: NodeConfig::default(),
            recent_events: VecDeque::new(),
            revision: 0,
        };
        d.node
            .set_protocols(protocol::EVENT_EXCHANGE | protocol::SNIP | protocol::DATAGRAM);
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
        for t in &layout.turntables {
            for (e, cw) in [(t.cw_event, true), (t.ccw_event, false)] {
                add(&mut self.index, e, Action::TurntableStep(t.id.clone(), cw));
                if let Some(e) = e {
                    self.node.add_producer(e, EventState::Unknown);
                }
            }
            for (i, tr) in t.tracks.iter().enumerate() {
                add(
                    &mut self.index,
                    tr.feedback_event,
                    Action::TurntableFeedback(t.id.clone(), i),
                );
                if let Some(e) = tr.feedback_event {
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
        let old_turntables = std::mem::take(&mut self.turntables);
        for t in &layout.turntables {
            let mut st = old_turntables.get(&t.id).copied().unwrap_or_default();
            // Track indexes change meaning if tracks were added or removed.
            let n = old_layout_tracks(&self.layout, &t.id);
            if n != Some(t.tracks.len()) {
                st = TurntableStatus::default();
            }
            self.turntables.insert(t.id.clone(), st);
        }
        self.pending_deadlines.retain(|id, _| {
            layout.turnout(id).is_some_and(|t| t.has_feedback())
                || layout.turntable(id).is_some()
                    && self.turntables.get(id).is_some_and(|s| s.pending.is_some())
        });
        for (id, st) in &mut self.turnouts {
            if st.pending.is_some() && !self.pending_deadlines.contains_key(id) {
                st.pending = None;
            }
        }
        for (id, st) in &mut self.turntables {
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
        self.requery = None;
        self.config.link_down();
        for b in self.blocks.values_mut() {
            *b = Occupancy::Unknown;
        }
        for t in self.turnouts.values_mut() {
            *t = TurnoutStatus::default();
        }
        for t in self.turntables.values_mut() {
            *t = TurntableStatus::default();
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
        let (pending, requery) = if self.link_up {
            (
                self.pending_deadlines.values().copied().min(),
                self.requery.map(|(at, _)| at),
            )
        } else {
            (None, None)
        };
        let config = if self.link_up {
            self.config.next_deadline()
        } else {
            None
        };
        throttles
            .chain(pending)
            .chain(requery)
            .chain(config)
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
            self.config.discover();
        }
        let busy = self.config.busy_total();
        self.config.poll(now);
        if self.config.busy_total() != busy {
            self.revision += 1;
        }
        self.flush_config();
        if let Some((at, left)) = self.requery {
            if now >= at {
                self.requery = None;
                if self.query_unknown() && left > 1 {
                    self.requery = Some((now + REQUERY_MS, left - 1));
                }
            }
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
            if let Some(st) = self.turntables.get_mut(&id) {
                if st.pending.take().is_some() {
                    st.stalled = true;
                    self.revision += 1;
                }
            }
        }
    }

    /// Ask the bus for the current state of every block, turnout and
    /// turntable. Done automatically after logging in (and again, for
    /// anything still unknown, [`REQUERY_MS`] later); call it to refresh.
    pub fn query_state(&mut self) {
        self.query(|_| true);
        self.requery = Some((self.now + REQUERY_MS, REQUERY_TRIES));
    }

    /// Query only what is still unknown. Returns false if nothing was.
    fn query_unknown(&mut self) -> bool {
        let unknown_blocks: Vec<String> = self
            .blocks
            .iter()
            .filter(|(_, o)| **o == Occupancy::Unknown)
            .map(|(id, _)| id.clone())
            .collect();
        let unknown_turnouts: Vec<String> = self
            .turnouts
            .iter()
            .filter(|(_, t)| t.position == Position::Unknown && t.pending.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let unknown_tables: Vec<String> = self
            .turntables
            .iter()
            .filter(|(_, t)| t.position.is_none() && t.pending.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        if unknown_blocks.is_empty() && unknown_turnouts.is_empty() && unknown_tables.is_empty() {
            return false;
        }
        self.query(|id| {
            unknown_blocks.iter().any(|b| b == id)
                || unknown_turnouts.iter().any(|t| t == id)
                || unknown_tables.iter().any(|t| t == id)
        });
        true
    }

    /// Send the state queries for the blocks, turnouts and turntables whose
    /// id passes `want`.
    fn query(&mut self, want: impl Fn(&str) -> bool) {
        // Producers: detectors and feedback sensors know the state. So do
        // other panels and JMRI for turnout commands they produce.
        let mut producers = Vec::new();
        // Consumers: turnout drivers report their current output.
        let mut consumers = Vec::new();
        for b in self.layout.blocks.iter().filter(|b| want(&b.id)) {
            producers.extend(b.occupied_event);
            producers.extend(b.clear_event);
        }
        for t in self.layout.turnouts.iter().filter(|t| want(&t.id)) {
            if let Some(fb) = &t.feedback {
                producers.extend(fb.normal_event);
                producers.extend(fb.reverse_event);
            }
            for e in [t.normal_event, t.reverse_event].into_iter().flatten() {
                producers.push(e);
                consumers.push(e);
            }
        }
        for t in self.layout.turntables.iter().filter(|t| want(&t.id)) {
            producers.extend(t.tracks.iter().filter_map(|tr| tr.feedback_event));
        }
        producers.sort_unstable();
        producers.dedup();
        consumers.sort_unstable();
        consumers.dedup();
        for e in producers {
            self.send(Message::IdentifyProducer(e));
        }
        for e in consumers {
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
        let config_changed = match me {
            Some(me) => self.config.receive(rx.src_alias, &rx.msg, me, now),
            None => false,
        };
        self.flush_config();
        config_changed | self.receive_event(rx, me, now)
    }

    fn receive_event(&mut self, rx: rolcb::Received, me: Option<u16>, now: u64) -> bool {
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

    fn flush_config(&mut self) {
        for m in self.config.take_outgoing() {
            self.send(m);
        }
    }

    /// Ask every node on the bus to identify itself, filling
    /// [`config.nodes`](NodeConfig::nodes) with their names. Done
    /// automatically after logging in.
    pub fn discover_nodes(&mut self) {
        self.revision += 1;
        self.config.discover();
        self.flush_config();
    }

    /// Read `node`'s CDI (replacing any copy already read).
    pub fn load_cdi(&mut self, node: NodeId) {
        self.revision += 1;
        self.config.enqueue_cdi(node, self.now);
        self.flush_config();
    }

    /// Read `len` bytes of `node`'s memory space `space` from `addr` into
    /// [`config.data`](NodeConfig::data).
    pub fn read_config(&mut self, node: NodeId, space: u8, addr: u32, len: u32) {
        self.revision += 1;
        self.config.enqueue_read(node, space, addr, len, self.now);
        self.flush_config();
    }

    /// Read every field described by `node`'s CDI (which must be loaded).
    pub fn read_all_config(&mut self, node: NodeId) {
        let fields: Vec<(u8, u32, u32)> = match self.config.data.get(&node) {
            Some(crate::config::ConfigData {
                cdi: Some(Ok(cdi)), ..
            }) => cdi
                .fields()
                .into_iter()
                .map(|f| (f.space, f.address, f.size))
                .collect(),
            _ => return,
        };
        self.read_config_fields(node, fields);
    }

    /// Read the given `(space, address, size)` ranges of `node`'s memory,
    /// batched into as few requests as possible.
    pub fn read_config_fields(&mut self, node: NodeId, fields: Vec<(u8, u32, u32)>) {
        for (space, addr, len) in plan_reads(fields) {
            self.config.enqueue_read(node, space, addr, len, self.now);
        }
        self.revision += 1;
        self.flush_config();
    }

    /// Write `data` to `node`'s memory space `space` at `addr`.
    pub fn write_config(&mut self, node: NodeId, space: u8, addr: u32, data: &[u8]) {
        self.revision += 1;
        self.config.enqueue_write(node, space, addr, data, self.now);
        self.flush_config();
    }

    /// Tell `node` a set of writes is finished (some nodes apply changes
    /// only then).
    pub fn config_update_complete(&mut self, node: NodeId) {
        self.revision += 1;
        self.config.enqueue_update_complete(node, self.now);
        self.flush_config();
    }

    /// Restart `node`, as configuration tools do after changes that need it.
    pub fn restart_node(&mut self, node: NodeId) {
        self.revision += 1;
        self.config.enqueue_reset(node, self.now);
        self.flush_config();
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
                        // The driver's output (or another panel's last
                        // command) is the best guess until feedback says.
                        (Report::ProducerValid | Report::ConsumerValid, true) => {
                            if let Some(t) = self.turnouts.get_mut(id) {
                                if t.position == Position::Unknown && t.pending.is_none() {
                                    t.position = p;
                                    changed = true;
                                }
                            }
                        }
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
                Action::TurntableFeedback(id, i) => {
                    if !matches!(report, Report::Pcer | Report::ProducerValid) {
                        continue;
                    }
                    if let Some(t) = self.turntables.get_mut(id) {
                        let before = *t;
                        t.position = Some(*i);
                        t.stalled = false;
                        if t.pending == Some(*i) {
                            t.pending = None;
                            self.pending_deadlines.remove(id);
                        } else if t.pending.is_some() {
                            // Passing a track on the way: still moving.
                            self.pending_deadlines
                                .insert(id.clone(), self.now + TURNTABLE_TIMEOUT_MS);
                        }
                        changed |= *t != before;
                    }
                }
                Action::TurntableStep(id, cw) => {
                    // Only a real report moves the bridge; Producer
                    // Identified is just someone announcing the event.
                    if report == Report::Pcer {
                        changed |= self.advance_turntable(id, if *cw { 1 } else { -1 });
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

    /// Account for the bridge moving `steps` tracks on from where it was
    /// heading. Unknown positions stay unknown.
    fn advance_turntable(&mut self, id: &str, steps: i32) -> bool {
        let Some(t) = self.layout.turntable(id) else {
            return false;
        };
        let Some(st) = self.turntables.get(id).copied() else {
            return false;
        };
        let Some(target) = st
            .pending
            .or(st.position)
            .and_then(|from| t.step_from(from, steps))
        else {
            return false;
        };
        self.head_for(id, target)
    }

    /// The bridge is on its way to track `target`: wait for its feedback if
    /// it has one (asking the sensor in case it is already there), or take
    /// it as reached.
    fn head_for(&mut self, id: &str, target: usize) -> bool {
        let fb = self
            .layout
            .turntable(id)
            .and_then(|t| t.tracks.get(target))
            .and_then(|tr| tr.feedback_event);
        let Some(st) = self.turntables.get_mut(id) else {
            return false;
        };
        let before = *st;
        match fb {
            Some(e) => {
                st.pending = Some(target);
                self.pending_deadlines
                    .insert(id.to_string(), self.now + TURNTABLE_TIMEOUT_MS);
                if before.pending != Some(target) {
                    self.send(Message::IdentifyProducer(e));
                }
            }
            None => {
                st.position = Some(target);
                st.pending = None;
                self.pending_deadlines.remove(id);
            }
        }
        self.turntables.get(id) != Some(&before)
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

    /// The positions a pair of turnouts are set for, or being thrown to:
    /// for a switch worked by two turnouts, match it against
    /// [`Shape::routes`](crate::Shape::routes).
    pub fn pair_positions(&self, left: &str, right: &str) -> (Position, Position) {
        let pos = |id: &str| {
            let st = self.turnouts.get(id).copied().unwrap_or_default();
            st.pending.unwrap_or(st.position)
        };
        (pos(left), pos(right))
    }

    /// Set a switch worked by two turnouts (a 3-way switch or slip) to the
    /// route after the one it is set for now, in
    /// [`Shape::routes`](crate::Shape::routes) order (the first route if
    /// it's unknown). Returns the positions thrown, or `None` if a command
    /// couldn't be sent.
    pub fn next_route(
        &mut self,
        shape: crate::Shape,
        left: &str,
        right: &str,
    ) -> Option<(Position, Position)> {
        let routes = shape.routes();
        let now = self.pair_positions(left, right);
        let next = routes
            .iter()
            .position(|(p, _)| *p == now)
            .map_or(0, |i| (i + 1) % routes.len().max(1));
        let (pos, _) = *routes.get(next)?;
        self.set_pair(left, right, pos).then_some(pos)
    }

    /// Throw two turnouts together, the one going to normal first so the
    /// points are never both reversed on the way. Returns false if a
    /// command couldn't be sent.
    pub fn set_pair(&mut self, left: &str, right: &str, (l, r): (Position, Position)) -> bool {
        let order = if l == Position::Normal {
            [(left, l), (right, r)]
        } else {
            [(right, r), (left, l)]
        };
        order.into_iter().all(|(id, p)| self.throw(id, p))
    }

    /// Turn a turntable one track clockwise (`cw`) or counter-clockwise by
    /// sending its `cw_event` or `ccw_event`. Returns false if that can't be
    /// sent right now (offline, or no such event).
    pub fn step_turntable(&mut self, id: &str, cw: bool) -> bool {
        self.revision += 1;
        if self.link_state() != LinkState::Online {
            return false;
        }
        let Some(t) = self.layout.turntable(id) else {
            return false;
        };
        let Some(event) = (if cw { t.cw_event } else { t.ccw_event }) else {
            return false;
        };
        self.send(Message::Pcer(event));
        self.advance_turntable(id, if cw { 1 } else { -1 });
        true
    }

    /// Turn a turntable to track `index` (into its
    /// [`tracks`](crate::layout::Turntable::tracks)) the shorter way round,
    /// by sending one step event per track passed. Returns false if that
    /// can't be done: offline, the bridge's position isn't known, there is
    /// no such track, or the step event for that direction is missing.
    pub fn turn_to(&mut self, id: &str, index: usize) -> bool {
        self.revision += 1;
        if self.link_state() != LinkState::Online {
            return false;
        }
        let Some(t) = self.layout.turntable(id).cloned() else {
            return false;
        };
        let st = self.turntables.get(id).copied().unwrap_or_default();
        let Some(from) = st.pending.or(st.position) else {
            return false;
        };
        if index >= t.tracks.len() {
            return false;
        }
        let steps = t.steps_between(from, index);
        let Some(event) = (if steps >= 0 { t.cw_event } else { t.ccw_event }) else {
            return false;
        };
        for _ in 0..steps.unsigned_abs() {
            self.send(Message::Pcer(event));
        }
        if steps != 0 || st.pending.is_some() {
            self.head_for(id, index);
        }
        true
    }

    /// Tell the dispatcher where a turntable's bridge is (or that it isn't
    /// known), e.g. after looking at the layout. Clears any pending turn.
    pub fn set_turntable_position(&mut self, id: &str, index: Option<usize>) {
        self.revision += 1;
        let n = self.layout.turntable(id).map_or(0, |t| t.tracks.len());
        if let Some(st) = self.turntables.get_mut(id) {
            st.position = index.filter(|i| *i < n);
            st.pending = None;
            st.stalled = false;
            self.pending_deadlines.remove(id);
        }
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
