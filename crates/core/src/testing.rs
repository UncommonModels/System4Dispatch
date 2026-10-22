//! Test helpers: run a [`Dispatcher`] against a [`Simulator`] (and any extra
//! nodes) on a virtual clock, with no I/O.
//!
//! ```
//! use s4d_core::testing::Bench;
//! use s4d_core::{LinkState, Occupancy};
//!
//! let mut b = Bench::new();          // the demo layout, simulator quiet
//! b.s.set_occupied("B2", true);
//! b.run(500);                        // log in and learn the layout state
//! assert_eq!(b.d.link_state(), LinkState::Online);
//! assert_eq!(b.d.blocks["B2"], Occupancy::Occupied);
//! ```

use rolcb::message::Received;
use rolcb::{CanFrame, Node, NodeId};

use crate::dispatcher::Dispatcher;
use crate::layout::Layout;
use crate::sim::Simulator;

/// The dispatcher's Node ID on the bench.
pub const BENCH_NODE_ID: NodeId = NodeId::new(0x0501_0101_2299);

/// A dispatcher, a simulator and optional extra nodes wired to one virtual
/// CAN bus. Every frame sent by one party is delivered to all the others.
pub struct Bench {
    /// The dispatcher under test (already [`link_up`](Dispatcher::link_up)).
    pub d: Dispatcher,
    /// The simulated layout and command station. Random occupancy is off.
    pub s: Simulator,
    /// Extra nodes, e.g. a second panel. Add them with [`Bench::add_node`].
    pub nodes: Vec<Node>,
    /// Messages each extra node received, in order (same index as `nodes`).
    pub inbox: Vec<Vec<Received>>,
    /// The virtual clock, in milliseconds.
    pub now: u64,
}

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Bench {
    /// A bench running the [demo layout](Layout::demo).
    pub fn new() -> Self {
        Self::with_layout(Layout::demo())
    }

    /// A bench where both the dispatcher and the simulator use `layout`.
    pub fn with_layout(layout: Layout) -> Self {
        let mut s = Simulator::new(layout.clone(), 0);
        s.random_occupancy = false;
        let mut d = Dispatcher::new(BENCH_NODE_ID, layout);
        d.link_up(0);
        Bench {
            d,
            s,
            nodes: Vec::new(),
            inbox: Vec::new(),
            now: 0,
        }
    }

    /// Replace the simulator with a fresh one for `layout` (random
    /// occupancy off), started at the current time.
    pub fn replace_simulator(&mut self, layout: Layout) {
        self.s = Simulator::new(layout, self.now);
        self.s.random_occupancy = false;
    }

    /// Start `node` on the bus and return its index in
    /// [`nodes`](Bench::nodes).
    pub fn add_node(&mut self, mut node: Node) -> usize {
        node.start(self.now);
        self.nodes.push(node);
        self.inbox.push(Vec::new());
        self.nodes.len() - 1
    }

    /// Deliver queued frames back and forth until everyone is quiet (at
    /// most a few rounds), without advancing the clock.
    pub fn settle(&mut self) {
        let now = self.now;
        for _ in 0..10 {
            let mut frames: Vec<(usize, CanFrame)> = Vec::new();
            frames.extend(self.d.take_outgoing().into_iter().map(|f| (0, f)));
            frames.extend(self.s.take_outgoing().into_iter().map(|f| (1, f)));
            for (i, n) in self.nodes.iter_mut().enumerate() {
                frames.extend(n.take_outgoing().into_iter().map(|f| (i + 2, f)));
            }
            if frames.is_empty() {
                break;
            }
            for (from, f) in &frames {
                if *from != 0 {
                    self.d.receive(f, now);
                }
                if *from != 1 {
                    self.s.receive(f, now);
                }
                for (i, n) in self.nodes.iter_mut().enumerate() {
                    if *from != i + 2 {
                        if let Some(r) = n.receive(f, now) {
                            self.inbox[i].push(r);
                        }
                    }
                }
            }
        }
    }

    /// Advance the clock by `ms` in 10 ms steps, polling everyone and
    /// delivering frames after each step.
    pub fn run(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 10;
            self.d.poll(self.now);
            self.s.poll(self.now);
            for n in &mut self.nodes {
                n.poll(self.now);
            }
            self.settle();
        }
    }
}
