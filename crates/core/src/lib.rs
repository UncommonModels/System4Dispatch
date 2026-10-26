//! System4Dispatch core: everything that isn't UI or I/O.
//!
//! * [`layout`]: the panel file format, track geometry and editor operations.
//! * [`dispatcher`]: block, turnout and turntable state and throttles over
//!   a `rolcb::Node`.
//! * [`cdi`] and [`config`]: reading other nodes' configuration
//!   descriptions (CDI) and reading and writing their configuration memory.
//! * [`sim`]: a simulated layout and command station for demos and tests.
//! * [`testing`]: a virtual bench that wires a dispatcher to a simulator.
//!
//! Everything here is sans-IO and runs on the host, so the whole protocol
//! behaviour can be tested without hardware. For example, throwing a
//! turnout and waiting for its feedback:
//!
//! ```
//! use s4d_core::testing::Bench;
//! use s4d_core::{Position, TurnoutStatus};
//!
//! let mut b = Bench::new(); // demo layout: T1 has feedback sensors
//! b.run(500);               // log in and learn the current state
//! assert!(b.d.throw("T1", Position::Reverse));
//! assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
//!
//! b.run(1_000);             // the simulated machine moves and reports
//! assert_eq!(b.s.turnout_position("T1"), Some(Position::Reverse));
//! assert_eq!(
//!     b.d.turnouts["T1"],
//!     TurnoutStatus { position: Position::Reverse, pending: None, stalled: false },
//! );
//! ```
//!
//! # Features
//!
//! * `schema`: derive [`schemars::JsonSchema`](https://docs.rs/schemars) for
//!   the layout types and enable [`layout::json_schema`].

#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod cdi;
pub mod config;
pub mod dispatcher;
pub mod layout;
pub mod sim;
pub mod testing;

pub use dispatcher::{
    Dispatcher, FailReason, LinkState, Occupancy, Position, Throttle, ThrottleState, TurnoutStatus,
    TurntableStatus,
};
pub use layout::{
    Block, Element, Feedback, Grid, Issue, Layout, MoveError, RouteError, Shape, Turnout,
    Turntable, TurntableTrack, FORMAT_VERSION,
};
