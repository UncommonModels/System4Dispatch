//! System4Dispatch core: everything that isn't UI or I/O.
//!
//! * [`layout`] — the panel file format and track geometry.
//! * [`dispatcher`] — block/turnout state and throttles over a `rolcb::Node`.
//! * [`sim`] — a simulated layout and command station for demos and tests.

pub mod dispatcher;
pub mod layout;
pub mod sim;

pub use dispatcher::{
    Dispatcher, LinkState, Occupancy, Position, Throttle, ThrottleState, TurnoutStatus,
};
pub use layout::{Element, Layout, Shape};
