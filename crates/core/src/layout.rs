//! The dispatch panel layout file: a grid of track tiles, plus the blocks and
//! turnouts they belong to and the OpenLCB events that drive them.
//!
//! # File format
//!
//! A layout is a JSON document that deserialises into [`Layout`]. Event IDs
//! are strings of eight hex bytes (`"05.01.01.01.22.00.00.10"`; the
//! separators `.`, `:`, `-` and space are optional). With the `schema`
//! feature, [`json_schema`] describes the format as a JSON Schema; the
//! committed copy lives at `docs/schema/layout-v1.schema.json`.
//!
//! ```
//! use s4d_core::Layout;
//!
//! let l = Layout::from_json(r#"{
//!     "name": "Tiny",
//!     "grid": { "cols": 4, "rows": 2 },
//!     "blocks": [{ "id": "B1", "occupied_event": "05.01.01.01.22.00.00.10" }],
//!     "elements": [{ "type": "track", "x": 0, "y": 0, "shape": "straight", "block": "B1" }]
//! }"#).unwrap();
//! assert_eq!(l.blocks[0].display_name(), "B1");
//! assert!(l.validate().is_empty());
//! ```
//!
//! # Directions and rotation
//!
//! Tile geometry uses eight compass directions numbered clockwise from
//! north: `0` = N, `1` = NE, `2` = E, `3` = SE, `4` = S, `5` = SW, `6` = W,
//! `7` = NW. The extra value [`CENTER`] (`8`) is the middle of the tile and
//! marks a dead end (a bumper). Screen coordinates grow to the right and
//! downwards, so N is the top edge of the tile (see [`dir_offset`]).
//!
//! An element's `rot` rotates its shape clockwise in 45° steps, so it is
//! meaningful in `0..=7`. [`Layout::from_json`] reduces larger values
//! modulo 8, and [`Layout::validate`] reports them in layouts built in code.
//!
//! # Legacy alias
//!
//! Early files (and the original design notes) wrote turnout tiles as
//! `{"type": "turnout", ...}`. That is accepted as an alias of
//! `"type": "track"`; whether a tile is a turnout depends only on its
//! [`Shape`]. Files are always written back with `"track"`.
//!
//! # Versioning
//!
//! [`Layout::version`] is the format version, currently [`FORMAT_VERSION`].
//! Adding optional fields (with serde defaults) does not change it, so older
//! files keep loading and newer files load in older apps with the new
//! fields ignored. The version is bumped only when an older app would
//! misread a file, and [`Layout::from_json`] refuses files whose version is
//! newer than it understands rather than silently dropping data.

use std::collections::BTreeSet;
use std::fmt;

use rolcb::EventId;

use crate::dispatcher::Position;
use serde::{Deserialize, Serialize};

/// The layout format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// Regular expression for an event ID string in the JSON Schema.
pub const EVENT_ID_PATTERN: &str = r"^[0-9A-Fa-f]{2}([.:\- ]?[0-9A-Fa-f]{2}){7}$";

/// A complete panel: grid size, blocks, turnouts and the tiles drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "System4Dispatch layout", extend("$id" = "layout-v1.schema.json"))
)]
pub struct Layout {
    /// Format version; see [the module docs](self#versioning).
    #[serde(default = "default_version")]
    #[cfg_attr(
        feature = "schema",
        schemars(
            description = "Format version. Files newer than the app understands are refused."
        )
    )]
    pub version: u32,
    /// Human-readable layout name, shown in the app's title bar.
    #[serde(default)]
    pub name: String,
    /// Size of the tile grid.
    #[serde(default)]
    pub grid: Grid,
    /// Detection blocks.
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Turnouts.
    #[serde(default)]
    pub turnouts: Vec<Turnout>,
    /// Turntables (drawn with [`Shape::Turntable`] tiles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turntables: Vec<Turntable>,
    /// Tiles drawn on the grid, at most one per cell.
    #[serde(default)]
    pub elements: Vec<Element>,
}

fn default_version() -> u32 {
    FORMAT_VERSION
}

/// The size of the tile grid, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Grid {
    /// Number of columns (x runs `0..cols`).
    pub cols: u16,
    /// Number of rows (y runs `0..rows`).
    pub rows: u16,
}

impl Default for Grid {
    fn default() -> Self {
        Grid { cols: 32, rows: 16 }
    }
}

/// A detection block. Occupancy detectors usually produce one event when the
/// block becomes occupied and another when it clears.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Block {
    /// Unique id (shared namespace with turnouts), e.g. `"B1"`.
    pub id: String,
    /// Display name; may be empty (see [`Block::display_name`]).
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(description = "Display name; the id is shown when empty.")
    )]
    pub name: String,
    /// Event produced when the block becomes occupied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub occupied_event: Option<EventId>,
    /// Event produced when the block becomes clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub clear_event: Option<EventId>,
    /// The detector that reports this block, if it was picked from a list
    /// of known devices (which also filled in the events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
}

/// The device output a block or turnout is wired to: which board, and which
/// of its outputs. See [`crate::femto`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Device {
    /// The device's model, e.g. `"FemtoLCC"`.
    pub model: String,
    /// The device's node ID, e.g. `"02.01.57.A7.CD.94"`.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub node: rolcb::NodeId,
    /// The output on the device: for a FemtoLCC `"A"`–`"D"`, or
    /// `"S<board>.<channel>"` for a servo board channel (see
    /// [`crate::femto::Output::code`]).
    pub output: String,
}

impl Block {
    /// The name to show: [`name`](Block::name), or the id if the name is empty.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// A turnout. Sending `normal_event` / `reverse_event` throws it; optional
/// feedback events report the real point position.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Turnout {
    /// Unique id (shared namespace with blocks), e.g. `"T1"`.
    pub id: String,
    /// Display name; may be empty (see [`Turnout::display_name`]).
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(description = "Display name; the id is shown when empty.")
    )]
    pub name: String,
    /// Event that throws the turnout to normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub normal_event: Option<EventId>,
    /// Event that throws the turnout to reverse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub reverse_event: Option<EventId>,
    /// Point-position feedback, if the turnout has sensors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<Feedback>,
    /// Another turnout that is always thrown together with this one (e.g.
    /// the other half of a crossover). Links are symmetric: both turnouts
    /// name each other. Use [`Layout::set_link`] to keep them consistent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        feature = "schema",
        schemars(
            description = "Id of a turnout always thrown together with this one (e.g. the other half of a crossover). Both turnouts must name each other and agree on `linked_inverted`."
        )
    )]
    pub linked: Option<String>,
    /// Throw the linked turnout to the opposite position instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linked_inverted: bool,
    /// The device output that drives this turnout, if it was picked from a
    /// list of known devices (which also filled in the events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
}

/// Events produced by a turnout's position sensors.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Feedback {
    /// Produced when the points reach normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub normal_event: Option<EventId>,
    /// Produced when the points reach reverse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub reverse_event: Option<EventId>,
}

impl Turnout {
    /// True if at least one feedback event is configured.
    pub fn has_feedback(&self) -> bool {
        self.feedback
            .as_ref()
            .is_some_and(|f| f.normal_event.is_some() || f.reverse_event.is_some())
    }

    /// The name to show: [`name`](Turnout::name), or the id if the name is empty.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// A turntable and its roundhouse: a pit drawn centred on a
/// [`Shape::Turntable`] tile, with tracks radiating from it at any angle.
/// Lead tracks join the rest of the panel; stalls are drawn inside an engine
/// house around the pit.
///
/// Like a turnout, it is driven by two events: `cw_event` turns the bridge
/// to the next track clockwise and `ccw_event` to the next one
/// counter-clockwise. A track's optional `feedback_event` reports that the
/// bridge has lined up with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Turntable {
    /// Unique id (shared namespace with blocks and turnouts), e.g. `"TT1"`.
    pub id: String,
    /// Display name; may be empty (see [`Turntable::display_name`]).
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(description = "Display name; the id is shown when empty.")
    )]
    pub name: String,
    /// Pit diameter in grid cells, at least 1.
    #[serde(default = "default_pit_size")]
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 8)))]
    pub size: u8,
    /// Event that turns the bridge to the next track clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub cw_event: Option<EventId>,
    /// Event that turns the bridge to the next track counter-clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub ccw_event: Option<EventId>,
    /// The tracks around the pit, in clockwise order from north (see
    /// [`Turntable::sort_tracks`]). The bridge stops at each in turn.
    #[serde(default)]
    pub tracks: Vec<TurntableTrack>,
}

fn default_pit_size() -> u8 {
    1
}

impl Default for Turntable {
    fn default() -> Self {
        Turntable {
            id: String::new(),
            name: String::new(),
            size: default_pit_size(),
            cw_event: None,
            ccw_event: None,
            tracks: Vec::new(),
        }
    }
}

/// A track leaving a turntable pit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurntableTrack {
    /// Direction from the pit's centre, in whole degrees clockwise from
    /// north (`0..360`), on the drawing as shown. Leads should use multiples
    /// of 45 so they meet the grid's track.
    #[cfg_attr(feature = "schema", schemars(range(min = 0, max = 359)))]
    pub angle: u16,
    /// Display name, e.g. `"Stall 3"`; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// A roundhouse stall (drawn inside the engine house) rather than a lead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stall: bool,
    /// Event produced when the bridge lines up with this track (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<EventIdSchema>"))]
    pub feedback_event: Option<EventId>,
}

/// Compass names for directions `0..=7`.
pub const DIR_NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

impl TurntableTrack {
    /// The name to show: [`name`](TurntableTrack::name), or the angle
    /// (`"45°"`) if the name is empty.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            format!("{}°", self.angle)
        } else {
            self.name.clone()
        }
    }

    /// The grid direction (`0..=7`) this track points in, if its angle is
    /// a multiple of 45°.
    pub fn dir(&self) -> Option<u8> {
        (self.angle % 45 == 0).then_some((self.angle / 45 % 8) as u8)
    }
}

impl Turntable {
    /// The name to show: [`name`](Turntable::name), or the id if the name is empty.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    /// Put the tracks in clockwise order from north and fold angles into
    /// `0..360`. The bridge's stops, and so track indexes, follow this order.
    pub fn sort_tracks(&mut self) {
        for t in &mut self.tracks {
            t.angle %= 360;
        }
        self.tracks.sort_by_key(|t| t.angle);
    }

    /// Add a track (keeping the order) unless one already has that angle.
    /// Returns its index, or `None` if the angle was taken.
    pub fn add_track(&mut self, angle: u16, name: &str, stall: bool) -> Option<usize> {
        let angle = angle % 360;
        if self.tracks.iter().any(|t| t.angle == angle) {
            return None;
        }
        self.tracks.push(TurntableTrack {
            angle,
            name: name.to_string(),
            stall,
            feedback_event: None,
        });
        self.sort_tracks();
        self.tracks.iter().position(|t| t.angle == angle)
    }

    /// The index of the track `steps` stops clockwise from `from`
    /// (counter-clockwise for negative `steps`).
    pub fn step_from(&self, from: usize, steps: i32) -> Option<usize> {
        let n = self.tracks.len() as i32;
        (n > 0).then(|| (from as i32 + steps).rem_euclid(n) as usize)
    }

    /// The shortest way from track `from` to track `to`: steps clockwise
    /// (positive) or counter-clockwise (negative). Ties go clockwise.
    ///
    /// ```
    /// use s4d_core::layout::Turntable;
    ///
    /// let mut t = Turntable::default();
    /// for a in [0, 30, 60, 90, 180, 270] {
    ///     t.add_track(a, "", true);
    /// }
    /// assert_eq!(t.steps_between(0, 2), 2);
    /// assert_eq!(t.steps_between(0, 5), -1);
    /// assert_eq!(t.steps_between(1, 4), 3);
    /// ```
    pub fn steps_between(&self, from: usize, to: usize) -> i32 {
        let n = self.tracks.len() as i32;
        if n == 0 {
            return 0;
        }
        let cw = (to as i32 - from as i32).rem_euclid(n);
        if cw <= n - cw {
            cw
        } else {
            cw - n
        }
    }

    /// An unused angle for a new stall: the middle of the widest gap
    /// between the existing tracks.
    pub fn free_stall_angle(&self) -> u16 {
        let mut angles: Vec<u16> = self.tracks.iter().map(|t| t.angle % 360).collect();
        angles.sort_unstable();
        angles.dedup();
        match angles.len() {
            0 => 90,
            1 => (angles[0] + 180) % 360,
            n => {
                let (gap, at) = (0..n)
                    .map(|i| {
                        let a = angles[i];
                        let b = if i + 1 < n {
                            angles[i + 1]
                        } else {
                            angles[0] + 360
                        };
                        (b - a, a)
                    })
                    .max()
                    .unwrap();
                (at + gap / 2) % 360
            }
        }
    }
}

/// Something drawn on the grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    /// A track tile. `"type": "turnout"` is accepted as a legacy alias.
    #[serde(alias = "turnout")]
    Track {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
        /// What is drawn.
        shape: Shape,
        /// Clockwise rotation in 45° steps, `0..=7`.
        #[serde(default)]
        #[cfg_attr(feature = "schema", schemars(range(min = 0, max = 7)))]
        rot: u8,
        /// The block this tile belongs to, by id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<String>,
        /// The block of the second track through a crossing tile
        /// ([`Shape::has_two_tracks`]): the track that doesn't start at the
        /// tile's W edge. `None` means the same block as `block`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block2: Option<String>,
        /// The turnout this tile shows, by id (turnout shapes only).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turnout: Option<String>,
        /// The second turnout of a switch worked by two
        /// ([`Shape::has_two_turnouts`]): the right-hand points of a 3-way
        /// switch or slip. `turnout` is the left-hand one. See
        /// [`Shape::routes`] for what each combination sets.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turnout2: Option<String>,
        /// The turntable this tile shows, by id ([`Shape::Turntable`] only).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turntable: Option<String>,
        /// The name of a [`Shape::Portal`]'s link: the track continues at
        /// the other portal with the same name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        portal: Option<String>,
        /// For a [`Shape::Crossing`] or [`Shape::CrossingDiag`]: the track
        /// that passes over the other on a bridge. `None` is a crossing at
        /// grade. The tracks connect the same way either way; only the
        /// drawing differs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        over: Option<Over>,
    },
    /// A text label.
    Label {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
        /// The text.
        text: String,
    },
}

impl Element {
    /// The cell this element occupies.
    pub fn pos(&self) -> (u16, u16) {
        match self {
            Element::Track { x, y, .. } | Element::Label { x, y, .. } => (*x, *y),
        }
    }

    /// Move the element to another cell.
    pub fn set_pos(&mut self, (nx, ny): (u16, u16)) {
        match self {
            Element::Track { x, y, .. } | Element::Label { x, y, .. } => {
                *x = nx;
                *y = ny;
            }
        }
    }
}

/// Which track of a crossing tile is on the bridge; see
/// [`Element::Track`]'s `over`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Over {
    /// The track from the W edge (`block`).
    First,
    /// The other track (`block2`).
    Second,
}

/// Tile shapes. Each can be rotated in 45° steps (`rot` 0..=7, clockwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// W to E.
    Straight,
    /// W to NE (a 45° curve). `"tight_curve"`, a 90° bend in some
    /// development builds, is read as this.
    #[serde(alias = "tight_curve")]
    Curve,
    /// W to a dead end.
    Bumper,
    /// W–E crossing N–S.
    Crossing,
    /// W–E crossing NW–SE (a diamond).
    CrossingDiag,
    /// Normal W–E, reverse W–NE.
    TurnoutLeft,
    /// Normal W–E, reverse W–SE.
    TurnoutRight,
    /// Normal W–NE, reverse W–SE.
    Wye,
    /// Three routes from W: left to NE, straight to E, right to SE. Thrown
    /// by two turnouts: `turnout` reversed takes the left route, `turnout2`
    /// reversed the right route, both normal the straight route.
    ThreeWay,
    /// A diamond crossing (W–E and NW–SE) with one slip road, W–SE. Worked
    /// by two turnouts; see [`Shape::routes`].
    SingleSlip,
    /// A diamond crossing (W–E and NW–SE) with both slip roads, W–SE and
    /// NW–E. Worked by two turnouts; see [`Shape::routes`].
    DoubleSlip,
    /// A turntable pit, with its roundhouse. Its tracks come from the
    /// [`Turntable`], not from the tile's rotation.
    Turntable,
    /// A track from W that leaves here and carries on at the other portal
    /// with the same name, like a tunnel that wraps from one edge of the
    /// panel to the other.
    Portal,
}

impl Shape {
    /// Every shape, in palette order.
    pub const ALL: [Shape; 13] = [
        Shape::Straight,
        Shape::Curve,
        Shape::Bumper,
        Shape::Crossing,
        Shape::CrossingDiag,
        Shape::TurnoutLeft,
        Shape::TurnoutRight,
        Shape::Wye,
        Shape::ThreeWay,
        Shape::SingleSlip,
        Shape::DoubleSlip,
        Shape::Turntable,
        Shape::Portal,
    ];

    /// True for shapes that need a [`Turnout`].
    pub fn is_turnout(self) -> bool {
        matches!(
            self,
            Shape::TurnoutLeft
                | Shape::TurnoutRight
                | Shape::Wye
                | Shape::ThreeWay
                | Shape::SingleSlip
                | Shape::DoubleSlip
        )
    }

    /// True for tiles where two separate tracks cross, each of which can be
    /// in its own block (`block` and `block2`): crossings, diamonds and
    /// slips.
    pub fn has_two_tracks(self) -> bool {
        matches!(
            self,
            Shape::Crossing | Shape::CrossingDiag | Shape::SingleSlip | Shape::DoubleSlip
        )
    }

    /// For a crossing tile, whether segment `seg` (before rotation) belongs
    /// to the second track (`block2`): those that don't touch the W edge.
    ///
    /// ```
    /// use s4d_core::Shape;
    ///
    /// assert!(!Shape::Crossing.on_second_track((6, 2)));
    /// assert!(Shape::Crossing.on_second_track((0, 4)));
    /// assert!(Shape::DoubleSlip.on_second_track((7, 2)));
    /// assert!(!Shape::Straight.on_second_track((0, 4)));
    /// ```
    pub fn on_second_track(self, (a, b): Seg) -> bool {
        self.has_two_tracks() && a != 6 && b != 6
    }

    /// True for plain crossings, whose tracks can also pass one over the
    /// other on a bridge instead of crossing at grade.
    pub fn can_bridge(self) -> bool {
        matches!(self, Shape::Crossing | Shape::CrossingDiag)
    }

    /// True for switches worked by two turnouts (`turnout` and
    /// `turnout2`): 3-way switches and slips.
    pub fn has_two_turnouts(self) -> bool {
        matches!(
            self,
            Shape::ThreeWay | Shape::SingleSlip | Shape::DoubleSlip
        )
    }

    /// The routes of a switch worked by two turnouts, in the order the panel
    /// steps through them: the (left-hand, right-hand) turnout positions
    /// that set each route, and the segment it connects (before rotation).
    /// Empty for other shapes.
    ///
    /// | Shape | Normal, normal | Reverse, normal | Normal, reverse | Reverse, reverse |
    /// |---|---|---|---|---|
    /// | 3-way | W–E | W–NE (left) | W–SE (right) | — |
    /// | Single slip | W–E | W–SE (slip) | — | NW–SE |
    /// | Double slip | W–E | W–SE (slip) | NW–E (slip) | NW–SE |
    ///
    /// ```
    /// use s4d_core::{Position::*, Shape};
    ///
    /// let r = Shape::DoubleSlip.routes();
    /// assert_eq!(r.len(), 4);
    /// assert_eq!(r[1], ((Reverse, Normal), (6, 3)));
    /// assert!(Shape::Straight.routes().is_empty());
    /// ```
    pub fn routes(self) -> &'static [((Position, Position), Seg)] {
        use Position::{Normal as N, Reverse as R};
        match self {
            Shape::ThreeWay => &[((N, N), (6, 2)), ((R, N), (6, 1)), ((N, R), (6, 3))],
            Shape::SingleSlip => &[((N, N), (6, 2)), ((R, N), (6, 3)), ((R, R), (7, 3))],
            Shape::DoubleSlip => &[
                ((N, N), (6, 2)),
                ((R, N), (6, 3)),
                ((N, R), (7, 2)),
                ((R, R), (7, 3)),
            ],
            _ => &[],
        }
    }

    /// True for slip roads, which are drawn bowed out beside the crossing
    /// so they can be told apart from it.
    pub fn is_slip_road(self, (a, b): Seg) -> bool {
        matches!(self, Shape::SingleSlip | Shape::DoubleSlip) && (a + 4) % 8 != b
    }

    /// True for [`Shape::Portal`], which needs a portal name.
    pub fn is_portal(self) -> bool {
        self == Shape::Portal
    }

    /// True for [`Shape::Turntable`], which needs a [`Turntable`].
    pub fn is_turntable(self) -> bool {
        self == Shape::Turntable
    }

    /// A short English name for palettes.
    pub fn label(self) -> &'static str {
        match self {
            Shape::Straight => "Straight",
            Shape::Curve => "Curve",
            Shape::Bumper => "Bumper",
            Shape::Crossing => "Crossing",
            Shape::CrossingDiag => "Diamond",
            Shape::TurnoutLeft => "Turnout L",
            Shape::TurnoutRight => "Turnout R",
            Shape::Wye => "Wye",
            Shape::ThreeWay => "3-way",
            Shape::SingleSlip => "Single slip",
            Shape::DoubleSlip => "Double slip",
            Shape::Turntable => "Turntable",
            Shape::Portal => "Portal",
        }
    }

    /// Track legs, before rotation. Directions are 0 = N, 1 = NE, ... 7 = NW,
    /// and [`CENTER`] for a dead end.
    ///
    /// ```
    /// use s4d_core::layout::{Shape, CENTER};
    ///
    /// // A right-hand turnout runs W–E (normal) and W–SE (reverse).
    /// let legs = Shape::TurnoutRight.legs();
    /// assert_eq!(legs.normal, vec![(6, 2)]);
    /// assert_eq!(legs.reverse, vec![(6, 3)]);
    /// // Rotated 90° clockwise it runs N–S and N–SW.
    /// let r = legs.rotated(2);
    /// assert_eq!(r.normal, vec![(0, 4)]);
    /// assert_eq!(r.reverse, vec![(0, 5)]);
    /// // CENTER never rotates, and any `rot` is taken modulo 8.
    /// assert_eq!(Shape::Bumper.legs().rotated(255).fixed, vec![(5, CENTER)]);
    /// ```
    pub fn legs(self) -> Legs {
        const W: u8 = 6;
        let seg = |a, b| vec![(a, b)];
        match self {
            Shape::Straight => Legs::fixed(seg(W, 2)),
            Shape::Curve => Legs::fixed(seg(W, 1)),
            Shape::Bumper => Legs::fixed(seg(W, CENTER)),
            Shape::Crossing => Legs::fixed(vec![(W, 2), (0, 4)]),
            Shape::CrossingDiag => Legs::fixed(vec![(W, 2), (7, 3)]),
            Shape::TurnoutLeft => Legs::switched(seg(W, 2), seg(W, 1)),
            Shape::TurnoutRight => Legs::switched(seg(W, 2), seg(W, 3)),
            Shape::Wye => Legs::switched(seg(W, 1), seg(W, 3)),
            Shape::ThreeWay => Legs::switched(seg(W, 2), vec![(W, 1), (W, 3)]),
            Shape::SingleSlip => Legs::switched(vec![(W, 2), (7, 3)], seg(W, 3)),
            Shape::DoubleSlip => Legs::switched(vec![(W, 2), (7, 3)], vec![(W, 3), (7, 2)]),
            Shape::Turntable => Legs::default(),
            Shape::Portal => Legs::fixed(seg(W, CENTER)),
        }
    }
}

/// The "direction" of a tile's centre, used for dead ends.
pub const CENTER: u8 = 8;

/// A track segment between two directions (see [the module docs](self#directions-and-rotation)).
pub type Seg = (u8, u8);

/// The track segments of a tile. Turnouts split into normal / reverse legs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Legs {
    /// Segments that are always connected.
    pub fixed: Vec<Seg>,
    /// Segments connected when a turnout is normal.
    pub normal: Vec<Seg>,
    /// Segments connected when a turnout is reversed.
    pub reverse: Vec<Seg>,
}

impl Legs {
    fn fixed(fixed: Vec<Seg>) -> Self {
        Legs {
            fixed,
            ..Default::default()
        }
    }

    fn switched(normal: Vec<Seg>, reverse: Vec<Seg>) -> Self {
        Legs {
            normal,
            reverse,
            ..Default::default()
        }
    }

    /// Rotate clockwise by `rot` × 45°. Any `rot` is accepted (taken modulo 8).
    pub fn rotated(&self, rot: u8) -> Legs {
        let r = |d: u8| {
            if d == CENTER {
                d
            } else {
                (d % 8 + rot % 8) % 8
            }
        };
        let map = |v: &Vec<Seg>| v.iter().map(|&(a, b)| (r(a), r(b))).collect();
        Legs {
            fixed: map(&self.fixed),
            normal: map(&self.normal),
            reverse: map(&self.reverse),
        }
    }
}

/// Offset from the tile centre (in tile units, y down) of the edge or corner
/// for `dir`. [`CENTER`] and invalid values map to the centre.
pub fn dir_offset(dir: u8) -> (f32, f32) {
    match dir {
        0 => (0.0, -0.5),
        1 => (0.5, -0.5),
        2 => (0.5, 0.0),
        3 => (0.5, 0.5),
        4 => (0.0, 0.5),
        5 => (-0.5, 0.5),
        6 => (-0.5, 0.0),
        7 => (-0.5, -0.5),
        _ => (0.0, 0.0),
    }
}

/// The grid step (dx, dy) to the neighbouring cell in direction `dir`
/// (`dir` is taken modulo 8).
///
/// ```
/// assert_eq!(s4d_core::layout::dir_step(1), (1, -1)); // NE: right and up
/// ```
pub fn dir_step(dir: u8) -> (i32, i32) {
    let (x, y) = dir_offset(dir % 8);
    ((x * 2.0) as i32, (y * 2.0) as i32)
}

/// What [`Layout::insert_row`] and [`Layout::insert_column`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inserted {
    /// Joining tiles placed in the new row or column.
    pub joined: usize,
    /// Tiles just before the new row or column whose track across it
    /// couldn't be joined (both sides are switches, or it would need a 90°
    /// bend). Their cells don't move.
    pub gaps: Vec<(u16, u16)>,
}

/// The directions track leaves tile `el` in, each with the block of the
/// track that leaves that way.
fn track_exits(el: &Element, layout: &Layout) -> Vec<(u8, Option<String>)> {
    match el {
        Element::Track {
            shape: Shape::Turntable,
            turntable,
            block,
            ..
        } => turntable
            .as_deref()
            .and_then(|t| layout.turntable(t))
            .map(|t| {
                t.tracks
                    .iter()
                    .filter(|tr| !tr.stall)
                    .filter_map(|tr| tr.dir())
                    .map(|d| (d, block.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        Element::Track {
            shape,
            rot,
            block,
            block2,
            ..
        } => {
            let legs = shape.legs().rotated(*rot);
            let mut out = Vec::new();
            for &(a, b) in legs.fixed.iter().chain(&legs.normal).chain(&legs.reverse) {
                let blk = if shape.on_second_track((a, b)) {
                    block2.clone().or_else(|| block.clone())
                } else {
                    block.clone()
                };
                for d in [a, b] {
                    if d < 8 {
                        out.push((d, blk.clone()));
                    }
                }
            }
            out
        }
        Element::Label { .. } => Vec::new(),
    }
}

/// A problem found by [`Layout::validate`], as an English sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue(pub String);

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why [`Layout::move_cells`] refused a move. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// The tile at this (original) cell would leave the grid.
    OutOfGrid {
        /// Column of the tile that would leave the grid.
        x: u16,
        /// Row of the tile that would leave the grid.
        y: u16,
    },
    /// A moved tile would land on this cell, which holds a tile that is not
    /// being moved.
    Overlap {
        /// Column of the blocking tile.
        x: u16,
        /// Row of the blocking tile.
        y: u16,
    },
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoveError::OutOfGrid { x, y } => write!(f, "tile at {x},{y} would leave the grid"),
            MoveError::Overlap { x, y } => write!(f, "cell {x},{y} is already in use"),
        }
    }
}

impl std::error::Error for MoveError {}

/// Why [`Layout::route_track`] found no route. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteError {
    /// An endpoint is outside the grid.
    OutsideGrid,
    /// Both endpoints are the same cell.
    SameCell,
    /// This endpoint has track (or a label) with nowhere to join new track.
    NoFreeEnd {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
    },
    /// Every way between the endpoints is blocked.
    NoRoute,
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::OutsideGrid => f.write_str("that point is outside the grid"),
            RouteError::SameCell => f.write_str("pick two different cells"),
            RouteError::NoFreeEnd { x, y } => {
                write!(f, "the track at {x},{y} has no free end to join")
            }
            RouteError::NoRoute => f.write_str("there's no clear way between those points"),
        }
    }
}

impl std::error::Error for RouteError {}

/// The plain track tile (straight or 45° curve) with track ends in
/// directions `a` and `b`. Bends sharper than 45° can't be drawn; they
/// (and `a == b`) get a straight tile through `a`.
///
/// ```
/// use s4d_core::layout::shape_for_exits;
/// use s4d_core::Shape;
///
/// assert_eq!(shape_for_exits(6, 2), (Shape::Straight, 0));
/// assert_eq!(shape_for_exits(1, 6), (Shape::Curve, 0)); // W to NE
/// assert_eq!(shape_for_exits(4, 6), (Shape::Straight, 2)); // a 90° bend: not drawable
/// ```
pub fn shape_for_exits(a: u8, b: u8) -> (Shape, u8) {
    let (a, b) = (a % 8, b % 8);
    let rot_from = |first: u8| (first + 2) % 8; // rotation taking W (6) to `first`
    match (b + 8 - a) % 8 {
        3 => (Shape::Curve, rot_from(a)),
        5 => (Shape::Curve, rot_from(b)),
        _ => (Shape::Straight, rot_from(a) % 4),
    }
}

impl Layout {
    /// A layout with no tiles, blocks or turnouts and the default grid.
    pub fn empty(name: &str) -> Layout {
        Layout {
            version: FORMAT_VERSION,
            name: name.to_string(),
            grid: Grid::default(),
            blocks: Vec::new(),
            turnouts: Vec::new(),
            turntables: Vec::new(),
            elements: Vec::new(),
        }
    }

    /// Parse a layout file. Rotations are reduced modulo 8.
    ///
    /// ```
    /// use s4d_core::{Element, Layout};
    ///
    /// let l = Layout::from_json(
    ///     r#"{"elements":[{"type":"track","x":1,"y":0,"shape":"curve","rot":10}]}"#,
    /// ).unwrap();
    /// assert!(matches!(l.elements[0], Element::Track { rot: 2, .. }));
    /// assert!(Layout::from_json(r#"{"version": 99}"#).is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a human-readable message if the JSON is malformed, doesn't
    /// match the format, or declares a [`version`](Layout::version) newer
    /// than [`FORMAT_VERSION`].
    pub fn from_json(s: &str) -> Result<Layout, String> {
        let mut l: Layout = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if l.version > FORMAT_VERSION {
            return Err(format!(
                "layout format version {} is newer than this app supports ({FORMAT_VERSION})",
                l.version
            ));
        }
        for el in &mut l.elements {
            if let Element::Track { rot, .. } = el {
                *rot %= 8;
            }
        }
        Ok(l)
    }

    /// Serialise as pretty-printed JSON.
    ///
    /// # Panics
    ///
    /// Never in practice: every field serialises infallibly.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("layout serializes")
    }

    /// The built-in demo layout (`crates/core/demo-layout.json`).
    ///
    /// # Panics
    ///
    /// Only if the embedded file is broken, which the test suite rules out.
    pub fn demo() -> Layout {
        Layout::from_json(include_str!("../demo-layout.json")).expect("demo layout parses")
    }

    /// The block with this id.
    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// The turnout with this id.
    pub fn turnout(&self, id: &str) -> Option<&Turnout> {
        self.turnouts.iter().find(|t| t.id == id)
    }

    /// The turntable with this id.
    pub fn turntable(&self, id: &str) -> Option<&Turntable> {
        self.turntables.iter().find(|t| t.id == id)
    }

    /// Index into [`elements`](Layout::elements) of the tile at a cell.
    pub fn element_at(&self, x: u16, y: u16) -> Option<usize> {
        self.elements.iter().position(|e| e.pos() == (x, y))
    }

    /// Place an element, replacing whatever was in that cell.
    pub fn put(&mut self, el: Element) {
        let (x, y) = el.pos();
        match self.element_at(x, y) {
            Some(i) => self.elements[i] = el,
            None => self.elements.push(el),
        }
    }

    /// Remove the element at a cell, if any.
    pub fn remove_at(&mut self, x: u16, y: u16) {
        self.elements.retain(|e| e.pos() != (x, y));
    }

    /// Next free id like "B7" / "T3" / "TT1".
    ///
    /// # Panics
    ///
    /// Never in practice (it would need billions of ids in use).
    pub fn next_id(&self, prefix: &str) -> String {
        let used = |id: &str| {
            self.blocks.iter().any(|b| b.id == id)
                || self.turnouts.iter().any(|t| t.id == id)
                || self.turntables.iter().any(|t| t.id == id)
        };
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| !used(id))
            .unwrap()
    }

    /// The turnout thrown together with `id`, and whether it goes to the
    /// opposite position. One-sided links in hand-edited files are
    /// tolerated: if only the partner names `id`, it still counts.
    ///
    /// ```
    /// # use s4d_core::Layout;
    /// let mut l = Layout::demo();
    /// l.set_link("T1", Some("T2"), true);
    /// assert_eq!(l.linked_partner("T2"), Some(("T1".to_string(), true)));
    /// l.set_link("T2", None, false);
    /// assert_eq!(l.linked_partner("T1"), None);
    /// ```
    pub fn linked_partner(&self, id: &str) -> Option<(String, bool)> {
        let t = self.turnout(id)?;
        if let Some(p) = &t.linked {
            if p != id && self.turnout(p).is_some() {
                return Some((p.clone(), t.linked_inverted));
            }
        }
        self.turnouts
            .iter()
            .find(|o| o.id != id && o.linked.as_deref() == Some(id))
            .map(|o| (o.id.clone(), o.linked_inverted))
    }

    /// Link two turnouts so they are always thrown together (or unlink `a`
    /// with `b = None`). Any previous partners of either are released, and
    /// both sides get the same `inverted` flag. See
    /// [`linked_partner`](Layout::linked_partner) for an example.
    pub fn set_link(&mut self, a: &str, b: Option<&str>, inverted: bool) {
        let b = b.filter(|b| *b != a);
        let involved: Vec<String> = [Some(a), b]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect();
        for t in &mut self.turnouts {
            let points_at_involved = t.linked.as_ref().is_some_and(|l| involved.contains(l));
            if involved.contains(&t.id) || points_at_involved {
                t.linked = None;
                t.linked_inverted = false;
            }
        }
        if let Some(b) = b {
            for (me, other) in [(a, b), (b, a)] {
                if let Some(t) = self.turnouts.iter_mut().find(|t| t.id == me) {
                    t.linked = Some(other.to_string());
                    t.linked_inverted = inverted;
                }
            }
        }
    }

    /// Report dangling references, duplicate ids, inconsistent links,
    /// out-of-range rotations and out-of-grid tiles. An empty list means the
    /// layout is fine.
    ///
    /// ```
    /// use s4d_core::{Element, Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.put(Element::Track {
    ///     x: 0, y: 0, shape: Shape::Straight, rot: 0,
    ///     block: Some("B9".into()), block2: None, turnout: None, turnout2: None,
    ///     turntable: None, portal: None, over: None,
    /// });
    /// let issues = l.validate();
    /// assert_eq!(issues.len(), 1);
    /// assert_eq!(issues[0].to_string(), "tile at 0,0 refers to unknown block B9");
    /// ```
    pub fn validate(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for id in self
            .blocks
            .iter()
            .map(|b| &b.id)
            .chain(self.turnouts.iter().map(|t| &t.id))
            .chain(self.turntables.iter().map(|t| &t.id))
        {
            if !seen.insert(id) {
                out.push(Issue(format!("duplicate id {id}")));
            }
        }
        for t in &self.turnouts {
            if let Some(l) = &t.linked {
                match self.turnout(l) {
                    None => out.push(Issue(format!(
                        "turnout {} is linked to unknown turnout {l}",
                        t.id
                    ))),
                    Some(_) if l == &t.id => {
                        out.push(Issue(format!("turnout {} is linked to itself", t.id)))
                    }
                    Some(o) if o.linked.as_deref().is_some_and(|x| x != t.id) => {
                        out.push(Issue(format!(
                            "turnout {} is linked to {l}, but {l} is linked to {}",
                            t.id,
                            o.linked.as_deref().unwrap_or("")
                        )))
                    }
                    Some(o)
                        if o.linked.as_deref() == Some(t.id.as_str())
                            && o.linked_inverted != t.linked_inverted
                            && t.id < o.id =>
                    {
                        out.push(Issue(format!(
                            "turnouts {} and {l} disagree on whether their link is inverted",
                            t.id
                        )))
                    }
                    _ => {}
                }
            }
        }
        for t in &self.turntables {
            let mut seen = BTreeSet::new();
            for tr in &t.tracks {
                if tr.angle >= 360 {
                    out.push(Issue(format!(
                        "turntable {} has a track at {}° (must be 0..360)",
                        t.id, tr.angle
                    )));
                } else if !seen.insert(tr.angle) {
                    out.push(Issue(format!(
                        "turntable {} has two tracks at {}°",
                        t.id, tr.angle
                    )));
                }
            }
            if t.tracks.windows(2).any(|w| w[0].angle > w[1].angle) {
                out.push(Issue(format!(
                    "turntable {}'s tracks are not in clockwise order",
                    t.id
                )));
            }
            if t.size == 0 {
                out.push(Issue(format!("turntable {} has size 0", t.id)));
            }
        }
        for el in &self.elements {
            let (x, y) = el.pos();
            if x >= self.grid.cols || y >= self.grid.rows {
                out.push(Issue(format!("tile at {x},{y} is outside the grid")));
            }
            if let Element::Track {
                block,
                block2,
                turnout,
                turnout2,
                turntable,
                portal,
                shape,
                rot,
                ..
            } = el
            {
                if let Some(b) = block2 {
                    if self.block(b).is_none() {
                        out.push(Issue(format!(
                            "tile at {x},{y} refers to unknown block {b}"
                        )));
                    }
                }
                if *rot > 7 {
                    out.push(Issue(format!(
                        "tile at {x},{y} has rotation {rot} (must be 0..=7)"
                    )));
                }
                if let Some(b) = block {
                    if self.block(b).is_none() {
                        out.push(Issue(format!(
                            "tile at {x},{y} refers to unknown block {b}"
                        )));
                    }
                }
                if let Some(t) = turnout {
                    if self.turnout(t).is_none() {
                        out.push(Issue(format!(
                            "tile at {x},{y} refers to unknown turnout {t}"
                        )));
                    }
                }
                if shape.is_turnout() && turnout.is_none() {
                    out.push(Issue(format!(
                        "turnout tile at {x},{y} has no turnout assigned"
                    )));
                }
                if let Some(t) = turnout2 {
                    if self.turnout(t).is_none() {
                        out.push(Issue(format!(
                            "tile at {x},{y} refers to unknown turnout {t}"
                        )));
                    }
                }
                if shape.has_two_turnouts() && turnout2.is_none() {
                    out.push(Issue(format!(
                        "{} tile at {x},{y} has no right-hand turnout assigned",
                        shape.label()
                    )));
                }
                if shape.is_portal() && portal.as_deref().is_none_or(str::is_empty) {
                    out.push(Issue(format!("portal at {x},{y} has no name")));
                }
                if let Some(t) = turntable {
                    if self.turntable(t).is_none() {
                        out.push(Issue(format!(
                            "tile at {x},{y} refers to unknown turntable {t}"
                        )));
                    }
                }
                if shape.is_turntable() && turntable.is_none() {
                    out.push(Issue(format!(
                        "turntable tile at {x},{y} has no turntable assigned"
                    )));
                }
            }
        }
        let mut portals: std::collections::BTreeMap<&str, usize> = Default::default();
        for el in &self.elements {
            if let Element::Track {
                shape: Shape::Portal,
                portal: Some(p),
                ..
            } = el
            {
                *portals.entry(p.as_str()).or_default() += 1;
            }
        }
        for (p, n) in portals {
            match n {
                2 => {}
                1 => out.push(Issue(format!("portal {p} has no other end"))),
                n => out.push(Issue(format!(
                    "portal {p} has {n} ends (it needs exactly 2)"
                ))),
            }
        }
        out
    }

    /// The cell of the other end of the portal at (x, y), if that tile is a
    /// [`Shape::Portal`] and its partner exists.
    ///
    /// ```
    /// use s4d_core::{Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.place_track(0, 3, Shape::Portal, 4, None); // leaves to the W
    /// l.place_track(31, 3, Shape::Portal, 0, None); // comes back from the E
    /// assert_eq!(l.portal_partner(0, 3), Some((31, 3)));
    /// assert!(l.validate().is_empty());
    /// ```
    pub fn portal_partner(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let name = match &self.elements[self.element_at(x, y)?] {
            Element::Track {
                shape: Shape::Portal,
                portal: Some(p),
                ..
            } => p,
            _ => return None,
        };
        self.elements.iter().find_map(|e| match e {
            Element::Track {
                shape: Shape::Portal,
                portal: Some(p),
                x: px,
                y: py,
                ..
            } if p == name && (*px, *py) != (x, y) => Some((*px, *py)),
            _ => None,
        })
    }

    /// Portal names in use, with how many ends each has.
    fn portal_ends(&self) -> std::collections::BTreeMap<String, usize> {
        let mut out = std::collections::BTreeMap::new();
        for el in &self.elements {
            if let Element::Track {
                shape: Shape::Portal,
                portal: Some(p),
                ..
            } = el
            {
                *out.entry(p.clone()).or_default() += 1;
            }
        }
        out
    }

    /// The next unused portal name: `A`, `B`, ... `Z`, `AA`, `AB`, ...
    pub fn next_portal_name(&self) -> String {
        let used = self.portal_ends();
        (0..)
            .map(|mut n: usize| {
                let mut s = String::new();
                loop {
                    s.insert(0, (b'A' + (n % 26) as u8) as char);
                    if n < 26 {
                        break s;
                    }
                    n = n / 26 - 1;
                }
            })
            .find(|s| !used.contains_key(s))
            .unwrap()
    }

    /// The portal name a portal tile at (x, y) should get: the one it
    /// already has, else a name with only one end so far (pairing the two),
    /// else a new name.
    fn portal_for(&self, x: u16, y: u16, shape: Shape) -> Option<String> {
        if !shape.is_portal() {
            return None;
        }
        if let Some(Element::Track {
            portal: Some(p), ..
        }) = self.element_at(x, y).map(|i| &self.elements[i])
        {
            return Some(p.clone());
        }
        let open = self
            .portal_ends()
            .into_iter()
            .find(|(_, n)| *n == 1)
            .map(|(p, _)| p);
        Some(open.unwrap_or_else(|| self.next_portal_name()))
    }

    // ---- Editor operations ----------------------------------------------

    fn in_grid(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.grid.cols as i32 && y < self.grid.rows as i32
    }

    /// Add a turnout with a fresh id and no events; returns the id.
    fn new_blank_turnout(&mut self) -> String {
        let id = self.next_id("T");
        self.turnouts.push(Turnout {
            id: id.clone(),
            ..Default::default()
        });
        id
    }

    /// The defined turnout already shown by the track tile at a cell.
    fn turnout_at(&self, x: u16, y: u16) -> Option<String> {
        let i = self.element_at(x, y)?;
        match &self.elements[i] {
            Element::Track {
                turnout: Some(t), ..
            } if self.turnout(t).is_some() => Some(t.clone()),
            _ => None,
        }
    }

    /// The turnout a tile at (x, y) with `shape` should show: the existing
    /// one, or a new blank one. `None` for non-turnout shapes.
    fn turnout_for(&mut self, x: u16, y: u16, shape: Shape) -> Option<String> {
        if !shape.is_turnout() {
            return None;
        }
        Some(
            self.turnout_at(x, y)
                .unwrap_or_else(|| self.new_blank_turnout()),
        )
    }

    /// The second turnout of a 3-way tile at a cell (kept, or a new blank
    /// one). `None` for other shapes.
    fn turnout2_for(&mut self, x: u16, y: u16, shape: Shape) -> Option<String> {
        if !shape.has_two_turnouts() {
            return None;
        }
        let existing = self.element_at(x, y).and_then(|i| match &self.elements[i] {
            Element::Track {
                turnout2: Some(t), ..
            } if self.turnout(t).is_some() => Some(t.clone()),
            _ => None,
        });
        Some(existing.unwrap_or_else(|| self.new_blank_turnout()))
    }

    /// The defined turntable already shown by the tile at a cell.
    fn turntable_at(&self, x: u16, y: u16) -> Option<String> {
        let i = self.element_at(x, y)?;
        match &self.elements[i] {
            Element::Track {
                turntable: Some(t), ..
            } if self.turntable(t).is_some() => Some(t.clone()),
            _ => None,
        }
    }

    /// The turntable a tile at (x, y) with `shape` should show: the
    /// existing one, or a new one with a lead for every track that already
    /// ends at the cell. `None` for other shapes.
    fn turntable_for(&mut self, x: u16, y: u16, shape: Shape) -> Option<String> {
        if !shape.is_turntable() {
            return None;
        }
        if let Some(t) = self.turntable_at(x, y) {
            return Some(t);
        }
        let id = self.next_id("TT");
        let mut t = Turntable {
            id: id.clone(),
            ..Default::default()
        };
        for d in self.connected_dirs(x, y) {
            t.add_track(d as u16 * 45, "", false);
        }
        self.turntables.push(t);
        Some(id)
    }

    /// Directions in which a neighbouring tile has track running into the
    /// cell (x, y), in direction order. Turntable neighbours count if they
    /// have a lead facing the cell.
    ///
    /// ```
    /// use s4d_core::{Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.place_track(4, 5, Shape::Straight, 0, None); // W of (5, 5)
    /// l.place_track(6, 4, Shape::Straight, 3, None); // NE of it, diagonal
    /// assert_eq!(l.connected_dirs(5, 5), vec![1, 6]);
    /// ```
    pub fn connected_dirs(&self, x: u16, y: u16) -> Vec<u8> {
        (0..8u8)
            .filter(|&d| {
                let (dx, dy) = dir_step(d);
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if !self.in_grid(nx, ny) {
                    return false;
                }
                let back = (d + 4) % 8;
                match self
                    .element_at(nx as u16, ny as u16)
                    .map(|i| &self.elements[i])
                {
                    Some(Element::Track {
                        shape: Shape::Turntable,
                        turntable,
                        ..
                    }) => turntable
                        .as_deref()
                        .and_then(|t| self.turntable(t))
                        .is_some_and(|t| {
                            t.tracks
                                .iter()
                                .any(|tr| !tr.stall && tr.dir() == Some(back))
                        }),
                    Some(Element::Track { shape, rot, .. }) => {
                        let legs = shape.legs().rotated(*rot);
                        legs.fixed
                            .iter()
                            .chain(&legs.normal)
                            .chain(&legs.reverse)
                            .any(|&(a, b)| a == back || b == back)
                    }
                    _ => false,
                }
            })
            .collect()
    }

    /// Give turntable `id` a lead for every track that ends at its tile and
    /// doesn't have one yet. Returns how many were added.
    pub fn add_connected_leads(&mut self, id: &str) -> usize {
        let cells: Vec<(u16, u16)> = self
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Track {
                    x,
                    y,
                    turntable: Some(t),
                    ..
                } if t == id => Some((*x, *y)),
                _ => None,
            })
            .collect();
        let dirs: Vec<u8> = cells
            .into_iter()
            .flat_map(|(x, y)| self.connected_dirs(x, y))
            .collect();
        let Some(t) = self.turntables.iter_mut().find(|t| t.id == id) else {
            return 0;
        };
        dirs.into_iter()
            .filter(|&d| t.add_track(d as u16 * 45, "", false).is_some())
            .count()
    }

    /// Delete a turntable and clear every tile's reference to it. Returns
    /// false if there was no such turntable.
    pub fn remove_turntable(&mut self, id: &str) -> bool {
        let before = self.turntables.len();
        self.turntables.retain(|t| t.id != id);
        if self.turntables.len() == before {
            return false;
        }
        for el in &mut self.elements {
            if let Element::Track { turntable, .. } = el {
                if turntable.as_deref() == Some(id) {
                    *turntable = None;
                }
            }
        }
        true
    }

    /// Draw a turntable and roundhouse at (x, y): a pit `size` cells
    /// across, a lead of `lead` straight tiles leaving it towards `rot` (0 =
    /// W, then clockwise in 45° steps like tile rotation), and `stalls`
    /// roundhouse stalls 15° apart around the far side. All tiles go in
    /// `block`. Returns the new turntable's id.
    ///
    /// ```
    /// use s4d_core::{Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// let tt = l.place_roundhouse(10, 8, 2, 0, 2, 6, None).unwrap();
    /// let t = l.turntable(&tt).unwrap();
    /// assert_eq!(t.tracks.len(), 7);
    /// assert_eq!(t.tracks.iter().filter(|t| t.stall).count(), 6);
    /// assert_eq!(t.tracks.iter().find(|t| !t.stall).unwrap().angle, 270); // lead to the W
    /// assert!(l.validate().is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// Refuses, changing nothing, with the first cell that is outside the
    /// grid or already in use.
    #[allow(clippy::too_many_arguments)]
    pub fn place_roundhouse(
        &mut self,
        x: u16,
        y: u16,
        size: u8,
        rot: u8,
        lead: u16,
        stalls: u16,
        block: Option<String>,
    ) -> Result<String, MoveError> {
        let rot = rot % 8;
        let size = size.max(1);
        let lead_dir = (6 + rot) % 8;
        // Lead tiles start outside the pit.
        let skip = (size as i32 - 1) / 2;
        let (dx, dy) = dir_step(lead_dir);
        let mut tiles = vec![((x as i32, y as i32), Shape::Turntable, 0)];
        for k in 1..=lead as i32 {
            let k = k + skip;
            // Straight track looks the same both ways round.
            tiles.push((
                (x as i32 + dx * k, y as i32 + dy * k),
                Shape::Straight,
                (lead_dir + 2) % 4,
            ));
        }
        for &((cx, cy), _, _) in &tiles {
            if !self.in_grid(cx, cy) {
                let (ox, oy) = (cx.clamp(0, u16::MAX as i32), cy.clamp(0, u16::MAX as i32));
                return Err(MoveError::OutOfGrid {
                    x: ox as u16,
                    y: oy as u16,
                });
            }
            if self.element_at(cx as u16, cy as u16).is_some() {
                return Err(MoveError::Overlap {
                    x: cx as u16,
                    y: cy as u16,
                });
            }
        }
        let id = self.next_id("TT");
        let mut tt = Turntable {
            id: id.clone(),
            size,
            ..Default::default()
        };
        let lead_angle = lead_dir as u16 * 45;
        tt.add_track(lead_angle, "Lead", false);
        let start = lead_angle + 180 + 360 - 15 * stalls.saturating_sub(1) / 2;
        for i in 0..stalls {
            tt.add_track(start + 15 * i, "", true);
        }
        // Number the stalls clockwise, starting after the lead.
        let lead_at = tt.tracks.iter().position(|t| !t.stall).unwrap_or(0);
        let n = tt.tracks.len();
        let mut k = 0;
        for i in 1..n {
            let t = &mut tt.tracks[(lead_at + i) % n];
            if t.stall {
                k += 1;
                t.name = format!("Stall {k}");
            }
        }
        self.turntables.push(tt);
        for ((cx, cy), shape, r) in tiles {
            self.put(Element::Track {
                x: cx as u16,
                y: cy as u16,
                shape,
                rot: r,
                block: block.clone(),
                turnout: None,
                turntable: shape.is_turntable().then(|| id.clone()),
                turnout2: None,
                block2: None,
                portal: None,
                over: None,
            });
        }
        Ok(id)
    }

    /// Directions in which the track tile at (x, y) has a track end that
    /// nothing joins yet: where new track can be attached. Empty for cells
    /// without track.
    pub fn free_ends(&self, x: u16, y: u16) -> Vec<u8> {
        let Some(el) = self.element_at(x, y).map(|i| &self.elements[i]) else {
            return Vec::new();
        };
        let ends: BTreeSet<u8> = match el {
            Element::Track {
                shape: Shape::Turntable,
                turntable,
                ..
            } => turntable
                .as_deref()
                .and_then(|t| self.turntable(t))
                .map(|t| {
                    t.tracks
                        .iter()
                        .filter(|tr| !tr.stall)
                        .filter_map(TurntableTrack::dir)
                        .collect()
                })
                .unwrap_or_default(),
            Element::Track {
                shape: Shape::Portal,
                ..
            } => BTreeSet::new(),
            Element::Track { shape, rot, .. } => {
                let legs = shape.legs().rotated(*rot);
                legs.fixed
                    .iter()
                    .chain(&legs.normal)
                    .chain(&legs.reverse)
                    .flat_map(|&(a, b)| [a, b])
                    .filter(|&d| d != CENTER)
                    .collect()
            }
            Element::Label { .. } => BTreeSet::new(),
        };
        let joined = self.connected_dirs(x, y);
        ends.into_iter().filter(|d| !joined.contains(d)).collect()
    }

    /// Work out new track joining cell `from` to cell `to` without
    /// touching anything already drawn, and return the tiles it would add
    /// (in `block`). Nothing is changed; see [`draw_route`](Layout::draw_route).
    ///
    /// An endpoint that already has track is joined at one of its
    /// [free ends](Layout::free_ends); an empty endpoint gets a new tile.
    /// The route takes empty cells only and prefers straight runs: 45°
    /// bends cost a little, and nothing sharper than 45° is used.
    ///
    /// ```
    /// use s4d_core::{Element, Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.place_track(2, 2, Shape::Bumper, 4, None);  // a line ending at (2, 2), open to the E
    /// l.place_track(12, 6, Shape::Bumper, 0, None); // another, open to the W
    /// let tiles = l.route_track((2, 2), (12, 6), None).unwrap();
    /// // Four cells diagonally (the rows differ by 4), the rest straight.
    /// assert_eq!(tiles.len(), 9);
    /// assert!(tiles.iter().all(|t| matches!(t,
    ///     Element::Track { shape: Shape::Straight | Shape::Curve, .. })));
    /// ```
    ///
    /// # Errors
    ///
    /// [`RouteError`] if an endpoint is outside the grid, is a cell with
    /// track but no free end, or no route fits.
    pub fn route_track(
        &self,
        from: (u16, u16),
        to: (u16, u16),
        block: Option<String>,
    ) -> Result<Vec<Element>, RouteError> {
        use std::cmp::Reverse;
        use std::collections::{BinaryHeap, HashMap};

        for &(x, y) in &[from, to] {
            if !self.in_grid(x as i32, y as i32) {
                return Err(RouteError::OutsideGrid);
            }
        }
        if from == to {
            return Err(RouteError::SameCell);
        }
        let from_ends = self.end_dirs(from)?;
        let to_ends = self.end_dirs(to)?;
        let from_empty = self.element_at(from.0, from.1).is_none();
        let to_empty = self.element_at(to.0, to.1).is_none();
        // Headings we may arrive at `to` with (moving into it).
        let arrive: Vec<u8> = match &to_ends {
            None => (0..8).collect(),
            Some(ends) => ends.iter().map(|e| (e + 4) % 8).collect(),
        };
        let step = |(x, y): (u16, u16), d: u8| {
            let (dx, dy) = dir_step(d);
            (x as i32 + dx, y as i32 + dy)
        };
        let free = |c: (i32, i32)| {
            self.in_grid(c.0, c.1) && self.element_at(c.0 as u16, c.1 as u16).is_none()
        };

        // State: a placed cell and the heading we entered it with.
        type State = ((u16, u16), u8);
        let mut dist: HashMap<State, u32> = HashMap::new();
        let mut prev: HashMap<State, State> = HashMap::new();
        let mut heap = BinaryHeap::new();
        let len = |d: u8| if d % 2 == 0 { 10 } else { 14 };
        match &from_ends {
            None => {
                for h in 0..8 {
                    dist.insert((from, h), 0);
                    heap.push(Reverse((0u32, from, h)));
                }
            }
            Some(ends) => {
                for &d in ends {
                    let n = step(from, d);
                    if (n.0 as u16, n.1 as u16) == to && n.0 >= 0 && n.1 >= 0 && arrive.contains(&d)
                    {
                        return Ok(Vec::new()); // already next to each other
                    }
                    if free(n) {
                        let s = ((n.0 as u16, n.1 as u16), d);
                        dist.insert(s, len(d));
                        heap.push(Reverse((len(d), s.0, d)));
                    }
                }
            }
        }
        let mut goal: Option<(State, u8)> = None;
        while let Some(Reverse((cost, cell, h))) = heap.pop() {
            if dist.get(&(cell, h)).is_some_and(|&c| c < cost) {
                continue;
            }
            if to_empty && cell == to {
                goal = Some(((cell, h), h));
                break;
            }
            let mut done = false;
            for turn in [0i8, -1, 1] {
                let h2 = (h as i8 + turn).rem_euclid(8) as u8;
                // The start cell (when empty) may leave in its own heading only.
                if from_empty && cell == from && turn != 0 {
                    continue;
                }
                let n = step(cell, h2);
                if n.0 < 0 || n.1 < 0 {
                    continue;
                }
                let nc = (n.0 as u16, n.1 as u16);
                let penalty = if turn == 0 { 0 } else { 4 };
                if !to_empty && nc == to {
                    if arrive.contains(&h2) {
                        goal = Some(((cell, h), h2));
                        done = true;
                        break;
                    }
                    continue;
                }
                if !(free(n) || (to_empty && nc == to)) {
                    continue;
                }
                let c2 = cost + len(h2) + penalty;
                if dist.get(&(nc, h2)).is_none_or(|&c| c2 < c) {
                    dist.insert((nc, h2), c2);
                    prev.insert((nc, h2), (cell, h));
                    heap.push(Reverse((c2, nc, h2)));
                }
            }
            if done {
                break;
            }
        }
        let ((mut cell, mut h), mut out_h) = goal.ok_or(RouteError::NoRoute)?;
        let mut tiles = Vec::new();
        loop {
            // A cell is entered heading `h` and left heading `out_h`.
            let (shape, rot) = shape_for_exits((h + 4) % 8, out_h);
            tiles.push(Element::Track {
                x: cell.0,
                y: cell.1,
                shape,
                rot,
                block: block.clone(),
                block2: None,
                turnout: None,
                turnout2: None,
                turntable: None,
                portal: None,
                over: None,
            });
            match prev.get(&(cell, h)) {
                Some(&(c, hp)) => {
                    out_h = h;
                    cell = c;
                    h = hp;
                }
                None => break,
            }
        }
        tiles.reverse();
        Ok(tiles)
    }

    /// Draw the track [`route_track`](Layout::route_track) finds, as one
    /// change. Returns the cells drawn on.
    ///
    /// # Errors
    ///
    /// As [`route_track`](Layout::route_track); nothing is changed.
    pub fn draw_route(
        &mut self,
        from: (u16, u16),
        to: (u16, u16),
        block: Option<String>,
    ) -> Result<BTreeSet<(u16, u16)>, RouteError> {
        let tiles = self.route_track(from, to, block)?;
        let cells = tiles.iter().map(Element::pos).collect();
        for t in tiles {
            self.put(t);
        }
        Ok(cells)
    }

    /// The directions a route may leave or enter cell `c` by: `None` for an
    /// empty cell (any), else its free ends.
    fn end_dirs(&self, (x, y): (u16, u16)) -> Result<Option<Vec<u8>>, RouteError> {
        match self.element_at(x, y).map(|i| &self.elements[i]) {
            None => Ok(None),
            Some(Element::Label { .. }) => Err(RouteError::NoFreeEnd { x, y }),
            Some(_) => {
                let ends = self.free_ends(x, y);
                if ends.is_empty() {
                    Err(RouteError::NoFreeEnd { x, y })
                } else {
                    Ok(Some(ends))
                }
            }
        }
    }

    /// Move the tiles in `cells` by (`dx`, `dy`). Empty cells in the
    /// selection are ignored. Returns the cells the tiles now occupy.
    ///
    /// # Errors
    ///
    /// Refuses (changing nothing) if a tile would leave the grid
    /// ([`MoveError::OutOfGrid`]) or land on a tile outside the selection
    /// ([`MoveError::Overlap`]).
    pub fn move_cells(
        &mut self,
        cells: &BTreeSet<(u16, u16)>,
        dx: i32,
        dy: i32,
    ) -> Result<BTreeSet<(u16, u16)>, MoveError> {
        let moving: Vec<usize> = (0..self.elements.len())
            .filter(|&i| cells.contains(&self.elements[i].pos()))
            .collect();
        let mut targets = Vec::with_capacity(moving.len());
        for &i in &moving {
            let (x, y) = self.elements[i].pos();
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if !self.in_grid(nx, ny) {
                return Err(MoveError::OutOfGrid { x, y });
            }
            targets.push((nx as u16, ny as u16));
        }
        for &t in &targets {
            if self
                .elements
                .iter()
                .any(|e| e.pos() == t && !cells.contains(&t))
            {
                return Err(MoveError::Overlap { x: t.0, y: t.1 });
            }
        }
        for (&i, &t) in moving.iter().zip(&targets) {
            self.elements[i].set_pos(t);
        }
        Ok(targets.into_iter().collect())
    }

    /// Insert an empty row at `y`: the tiles in row `y` and below move down
    /// one row and the grid grows by a row. Track that ran across the gap
    /// is joined up with a tile in the new row, in the block of the track
    /// above it (or below it, when the track above is a switch or a
    /// turntable): a straight where it ran straight down, and a curve where
    /// it ran diagonally, with the plain track tile below (or, if that's a
    /// switch, above) turned into a curve too so the line carries on. Where
    /// neither side can bend (two switches, or a curve that would have to
    /// turn 90°) the track is left for you to join: see [`Inserted`].
    /// Returns `None` (changing nothing) if `y` is past the end of the grid
    /// or the grid can't grow.
    ///
    /// ```
    /// use s4d_core::{Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.place_track(5, 3, Shape::Straight, 2, None); // N–S
    /// l.place_track(5, 4, Shape::Straight, 2, None);
    /// assert_eq!(l.insert_row(4).unwrap().joined, 1);
    /// assert_eq!(l.connected_dirs(5, 4), vec![0, 4]); // joined both ways
    /// assert!(l.element_at(5, 5).is_some());
    /// ```
    pub fn insert_row(&mut self, y: u16) -> Option<Inserted> {
        self.insert_line(y, false)
    }

    /// Insert an empty column at `x`: the tiles in column `x` and to its
    /// right move one column right. Otherwise like
    /// [`insert_row`](Layout::insert_row).
    pub fn insert_column(&mut self, x: u16) -> Option<Inserted> {
        self.insert_line(x, true)
    }

    fn insert_line(&mut self, at: u16, column: bool) -> Option<Inserted> {
        let size = if column {
            self.grid.cols
        } else {
            self.grid.rows
        };
        if at > size || size == u16::MAX {
            return None;
        }
        // The direction across the new line (S for a row, E for a column)
        // and a cell's coordinate across it.
        let fwd = if column { 2 } else { 4 };
        let across = |(x, y): (u16, u16)| if column { x } else { y };
        // Before moving anything: each connection from the line before
        // `at` to the line at `at` becomes a joining tile in the new line.
        // Straight across, it simply fills the gap. A diagonal can't: one
        // of its two tiles has to bend to meet the new line square, so the
        // tile on the far side bends if it's plain track, else the near
        // one does. `bend` is the bent tile's final cell and new shape.
        struct Join {
            cell: (u16, u16),
            exits: (u8, u8),
            block: Option<String>,
            bend: Option<((u16, u16), (Shape, u8))>,
        }
        let entry = (fwd + 4) % 8;
        // The other end of a plain track tile, given one end.
        let other_end = |el: Option<&Element>, end: u8| match el {
            Some(Element::Track { shape, rot, .. })
                if matches!(shape, Shape::Straight | Shape::Curve) =>
            {
                shape
                    .legs()
                    .rotated(*rot)
                    .fixed
                    .first()
                    .map(|&(a, b)| if a == end { b } else { a })
            }
            _ => None,
        };
        // A tile can run between these ends (straight or a 45° curve).
        let drawable = |a: u8, b: u8| (3..=5).contains(&((b + 8 - a) % 8));
        let mut joins: Vec<Join> = Vec::new();
        let mut gaps = Vec::new();
        if at > 0 {
            for el in &self.elements {
                let (x, y) = el.pos();
                if across((x, y)) != at - 1 {
                    continue;
                }
                let linked = self.connected_dirs(x, y);
                for (d, block) in track_exits(el, self) {
                    let (dx, dy) = dir_step(d);
                    if (if column { dx } else { dy }) != 1 || !linked.contains(&d) {
                        continue;
                    }
                    let step = |(cx, cy): (u16, u16), (sx, sy): (i32, i32)| {
                        ((cx as i32 + sx) as u16, (cy as i32 + sy) as u16)
                    };
                    let far_cell = step((x, y), (dx, dy));
                    let back = (d + 4) % 8;
                    // Track leaving a switch or turntable belongs to the
                    // block beyond it, if that has one.
                    let block = match (
                        el,
                        self.element_at(far_cell.0, far_cell.1)
                            .map(|i| &self.elements[i]),
                    ) {
                        (
                            Element::Track { shape, .. },
                            Some(Element::Track { block: Some(b), .. }),
                        ) if shape.is_turnout() || shape.is_turntable() => Some(b.clone()),
                        _ => block,
                    };
                    let join = if d == fwd {
                        Some(Join {
                            cell: far_cell,
                            exits: (back, fwd),
                            block,
                            bend: None,
                        })
                    } else {
                        let far = self
                            .element_at(far_cell.0, far_cell.1)
                            .map(|i| &self.elements[i]);
                        match (other_end(far, back), other_end(Some(el), d)) {
                            // The far tile turns to come in square.
                            (Some(o), _) if drawable(entry, o) => Some(Join {
                                cell: far_cell,
                                exits: (back, fwd),
                                block,
                                bend: Some((
                                    step(far_cell, dir_step(fwd)),
                                    shape_for_exits(entry, o),
                                )),
                            }),
                            // The near tile turns to leave square.
                            (_, Some(o)) if drawable(o, fwd) => Some(Join {
                                cell: step((x, y), dir_step(fwd)),
                                exits: (entry, d),
                                block,
                                bend: Some(((x, y), shape_for_exits(o, fwd))),
                            }),
                            _ => None,
                        }
                    };
                    match join {
                        Some(j) if !joins.iter().any(|k| k.cell == j.cell) => joins.push(j),
                        Some(_) => {}
                        None if !gaps.contains(&(x, y)) => gaps.push((x, y)),
                        None => {}
                    }
                }
            }
        }
        for el in &mut self.elements {
            let (x, y) = el.pos();
            if across((x, y)) >= at {
                el.set_pos(if column { (x + 1, y) } else { (x, y + 1) });
            }
        }
        if column {
            self.grid.cols += 1;
        } else {
            self.grid.rows += 1;
        }
        for j in &joins {
            let (shape, rot) = shape_for_exits(j.exits.0, j.exits.1);
            self.place_track(j.cell.0, j.cell.1, shape, rot, j.block.clone());
            if let Some(((bx, by), (new_shape, new_rot))) = j.bend {
                if let Some(i) = self.element_at(bx, by) {
                    if let Element::Track { shape, rot, .. } = &mut self.elements[i] {
                        (*shape, *rot) = (new_shape, new_rot);
                    }
                }
            }
        }
        Some(Inserted {
            joined: joins.len(),
            gaps,
        })
    }

    /// Remove every tile in `cells`. Block and turnout definitions are kept.
    /// Returns how many tiles were removed.
    pub fn remove_cells(&mut self, cells: &BTreeSet<(u16, u16)>) -> usize {
        let before = self.elements.len();
        self.elements.retain(|e| !cells.contains(&e.pos()));
        before - self.elements.len()
    }

    /// Rotate each track tile in `cells` clockwise by `delta` × 45° about its
    /// own centre (labels are unaffected). Use `delta = 7` for one step
    /// anticlockwise. A turntable tile turns its turntable's tracks
    /// instead, since those are stored as angles on the drawing.
    pub fn rotate_cells(&mut self, cells: &BTreeSet<(u16, u16)>, delta: u8) {
        let mut tables = BTreeSet::new();
        for el in &mut self.elements {
            if !cells.contains(&el.pos()) {
                continue;
            }
            match el {
                Element::Track {
                    shape: Shape::Turntable,
                    turntable,
                    ..
                } => tables.extend(turntable.clone()),
                Element::Track { rot, .. } => *rot = (*rot % 8 + delta % 8) % 8,
                Element::Label { .. } => {}
            }
        }
        for t in self
            .turntables
            .iter_mut()
            .filter(|t| tables.contains(&t.id))
        {
            for tr in &mut t.tracks {
                tr.angle = (tr.angle % 360) + (delta % 8) as u16 * 45;
            }
            t.sort_tracks();
        }
    }

    /// Put the track tiles in `cells` into `block` (or no block with
    /// `None`). Labels are skipped. Returns how many tiles were updated.
    pub fn assign_block(&mut self, cells: &BTreeSet<(u16, u16)>, block: Option<&str>) -> usize {
        let mut n = 0;
        for el in &mut self.elements {
            if !cells.contains(&el.pos()) {
                continue;
            }
            if let Element::Track { block: b, .. } = el {
                *b = block.map(str::to_string);
                n += 1;
            }
        }
        n
    }

    /// Delete a block and clear every tile's reference to it. Returns false
    /// if there was no such block.
    pub fn remove_block(&mut self, id: &str) -> bool {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.id != id);
        if self.blocks.len() == before {
            return false;
        }
        for el in &mut self.elements {
            if let Element::Track { block, block2, .. } = el {
                if block.as_deref() == Some(id) {
                    *block = None;
                }
                if block2.as_deref() == Some(id) {
                    *block2 = None;
                }
            }
        }
        true
    }

    /// Delete a turnout, clear every tile's reference to it and unlink its
    /// partner. Returns false if there was no such turnout.
    pub fn remove_turnout(&mut self, id: &str) -> bool {
        let before = self.turnouts.len();
        self.turnouts.retain(|t| t.id != id);
        if self.turnouts.len() == before {
            return false;
        }
        for t in &mut self.turnouts {
            if t.linked.as_deref() == Some(id) {
                t.linked = None;
                t.linked_inverted = false;
            }
        }
        for el in &mut self.elements {
            if let Element::Track {
                turnout, turnout2, ..
            } = el
            {
                if turnout.as_deref() == Some(id) {
                    *turnout = None;
                }
                if turnout2.as_deref() == Some(id) {
                    *turnout2 = None;
                }
            }
        }
        true
    }

    /// Draw a track tile at (x, y), replacing whatever is there.
    ///
    /// For turnout shapes the tile keeps the turnout already shown on that
    /// cell, or gets a new blank turnout (added to
    /// [`turnouts`](Layout::turnouts)); the turnout id is returned. Other
    /// shapes return `None`. A [`Shape::Turntable`] likewise keeps or gets a
    /// [`Turntable`], new ones with a position for each track already
    /// ending at the cell (see [`connected_dirs`](Layout::connected_dirs)).
    /// A switch worked by two turnouts (3-way, slips) also keeps or gets a
    /// second turnout, and a
    /// [`Shape::Portal`] keeps its name or takes the first name that has
    /// only one end (so placing two portals in a row pairs them), else a new
    /// one. Cells outside the grid are ignored (nothing is placed, `None` is
    /// returned). `rot` is taken modulo 8.
    pub fn place_track(
        &mut self,
        x: u16,
        y: u16,
        shape: Shape,
        rot: u8,
        block: Option<String>,
    ) -> Option<String> {
        if !self.in_grid(x as i32, y as i32) {
            return None;
        }
        let turnout = self.turnout_for(x, y, shape);
        let turnout2 = self.turnout2_for(x, y, shape);
        let turntable = self.turntable_for(x, y, shape);
        let portal = self.portal_for(x, y, shape);
        let (block2, over) = match self.element_at(x, y).map(|i| &self.elements[i]) {
            Some(Element::Track { block2, over, .. }) => (
                block2.clone().filter(|_| shape.has_two_tracks()),
                over.filter(|_| shape.can_bridge()),
            ),
            _ => (None, None),
        };
        // Turntables keep their geometry in their tracks, not the tile.
        let rot = if shape.is_turntable() { 0 } else { rot % 8 };
        self.put(Element::Track {
            x,
            y,
            shape,
            rot,
            block,
            block2,
            turnout: turnout.clone(),
            turnout2,
            turntable,
            portal,
            over,
        });
        turnout
    }

    /// Give a new piece of track the block it's laid in. Starting from the
    /// plain track tile (straight or curve, with no block) at (x, y), follow
    /// the line both ways through other plain tiles with no block. If both
    /// ends run into track in the same block, every tile of the line joins
    /// that block, which is returned. Otherwise (an end that's free, a
    /// switch with no block, or two different blocks) nothing changes.
    ///
    /// ```
    /// use s4d_core::{Block, Element, Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.blocks.push(Block { id: "B1".into(), ..Default::default() });
    /// l.place_track(1, 1, Shape::Straight, 0, Some("B1".into()));
    /// l.place_track(4, 1, Shape::Straight, 0, Some("B1".into()));
    /// l.place_track(2, 1, Shape::Straight, 0, None);
    /// assert_eq!(l.adopt_block(2, 1), None); // E end is still free
    /// l.place_track(3, 1, Shape::Straight, 0, None);
    /// assert_eq!(l.adopt_block(3, 1).as_deref(), Some("B1"));
    /// assert!(matches!(&l.elements[l.element_at(2, 1).unwrap()],
    ///     Element::Track { block: Some(b), .. } if b == "B1"));
    /// ```
    pub fn adopt_block(&mut self, x: u16, y: u16) -> Option<String> {
        let plain =
            |l: &Layout, (x, y): (u16, u16)| match l.element_at(x, y).map(|i| &l.elements[i]) {
                Some(
                    el @ Element::Track {
                        shape: Shape::Straight | Shape::Curve,
                        block: None,
                        ..
                    },
                ) => Some(
                    track_exits(el, l)
                        .into_iter()
                        .map(|(d, _)| d)
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            };
        let ends = plain(self, (x, y))?;
        let mut run = vec![(x, y)];
        let mut found: Vec<String> = Vec::new();
        for start in ends {
            // Walk from (x, y) out through `start` until the line reaches
            // track with a block, or stops.
            let (mut at, mut dir) = ((x, y), start);
            let block = loop {
                let (dx, dy) = dir_step(dir);
                let (nx, ny) = (at.0 as i32 + dx, at.1 as i32 + dy);
                if !self.in_grid(nx, ny) {
                    break None;
                }
                let next = (nx as u16, ny as u16);
                let back = (dir + 4) % 8;
                let Some(el) = self.element_at(next.0, next.1).map(|i| &self.elements[i]) else {
                    break None;
                };
                let exits = track_exits(el, self);
                let Some((_, leg_block)) = exits.iter().find(|(d, _)| *d == back) else {
                    break None;
                };
                if let Some(b) = leg_block {
                    break Some(b.clone());
                }
                match plain(self, next) {
                    Some(e) if !run.contains(&next) => {
                        run.push(next);
                        at = next;
                        dir = match e.into_iter().find(|&d| d != back) {
                            Some(d) => d,
                            None => break None,
                        };
                    }
                    _ => break None,
                }
            };
            found.push(block?);
        }
        let block = found.first()?.clone();
        if found.len() != 2 || found.iter().any(|b| *b != block) {
            return None;
        }
        for (cx, cy) in run {
            if let Some(i) = self.element_at(cx, cy) {
                if let Element::Track { block: b, .. } = &mut self.elements[i] {
                    *b = Some(block.clone());
                }
            }
        }
        Some(block)
    }

    /// Change the shape of the track tile at (x, y), keeping its rotation
    /// and block. Turnouts follow the same rule as
    /// [`place_track`](Layout::place_track): an existing turnout is kept, a
    /// blank one is created if needed, and non-turnout shapes drop the
    /// reference. Returns the tile's turnout id; does nothing (returning
    /// `None`) if the cell holds no track tile.
    pub fn set_tile_shape(&mut self, x: u16, y: u16, shape: Shape) -> Option<String> {
        let i = self.element_at(x, y)?;
        if !matches!(self.elements[i], Element::Track { .. }) {
            return None;
        }
        let id = self.turnout_for(x, y, shape);
        let id2 = self.turnout2_for(x, y, shape);
        let table = self.turntable_for(x, y, shape);
        let name = self.portal_for(x, y, shape);
        if let Element::Track {
            shape: s,
            block2,
            turnout,
            turnout2,
            turntable,
            portal,
            rot,
            over,
            ..
        } = &mut self.elements[i]
        {
            *s = shape;
            if !shape.can_bridge() {
                *over = None;
            }
            *turnout = id.clone();
            *turnout2 = id2;
            if !shape.has_two_tracks() {
                *block2 = None;
            }
            *turntable = table;
            *portal = name;
            if shape.is_turntable() {
                *rot = 0;
            }
        }
        id
    }

    /// Step the crossing at (x, y) through its options: at grade, the first
    /// track over the second on a bridge, then the second over the first.
    /// Returns the new setting, or `None` (changing nothing) if the cell
    /// holds no crossing that can be bridged.
    ///
    /// ```
    /// use s4d_core::layout::Over;
    /// use s4d_core::{Element, Layout, Shape};
    ///
    /// let mut l = Layout::empty("t");
    /// l.place_track(2, 2, Shape::Crossing, 0, None);
    /// assert_eq!(l.cycle_bridge(2, 2), Some(Some(Over::First)));
    /// assert_eq!(l.cycle_bridge(2, 2), Some(Some(Over::Second)));
    /// assert_eq!(l.cycle_bridge(2, 2), Some(None));
    /// l.place_track(3, 2, Shape::Straight, 0, None);
    /// assert_eq!(l.cycle_bridge(3, 2), None);
    /// ```
    pub fn cycle_bridge(&mut self, x: u16, y: u16) -> Option<Option<Over>> {
        let i = self.element_at(x, y)?;
        match &mut self.elements[i] {
            Element::Track { shape, over, .. } if shape.can_bridge() => {
                *over = match *over {
                    None => Some(Over::First),
                    Some(Over::First) => Some(Over::Second),
                    Some(Over::Second) => None,
                };
                Some(*over)
            }
            _ => None,
        }
    }

    /// Paste copied tiles, shifted diagonally by the first offset `k` in
    /// `1..=16` (both x and y move by `k`) at which every tile lands inside
    /// the grid on an empty cell. Turnout tiles get new blank turnouts,
    /// turntable tiles a copy of their turntable without events, and
    /// portals new names (portals pasted together stay paired);
    /// references to blocks that don't exist in this layout are dropped.
    /// Returns the cells pasted to, or `None` (changing nothing) if no
    /// offset fits.
    pub fn paste(&mut self, tiles: &[Element]) -> Option<BTreeSet<(u16, u16)>> {
        let occupied: BTreeSet<(u16, u16)> = self.elements.iter().map(Element::pos).collect();
        let k = (1..=16i32).find(|&k| {
            tiles.iter().all(|t| {
                let (x, y) = t.pos();
                let (nx, ny) = (x as i32 + k, y as i32 + k);
                self.in_grid(nx, ny) && !occupied.contains(&(nx as u16, ny as u16))
            })
        })? as u16;
        // Portals pasted together stay paired with each other, under new names.
        let mut portal_names: std::collections::BTreeMap<String, String> = Default::default();
        let mut placed = BTreeSet::new();
        for t in tiles {
            let mut el = t.clone();
            let (x, y) = el.pos();
            el.set_pos((x + k, y + k));
            if let Element::Track {
                shape,
                block,
                block2,
                turnout,
                turnout2,
                turntable,
                portal,
                rot,
                ..
            } = &mut el
            {
                *rot %= 8;
                *turnout = if shape.is_turnout() {
                    Some(self.new_blank_turnout())
                } else {
                    None
                };
                *turnout2 = if shape.has_two_turnouts() {
                    Some(self.new_blank_turnout())
                } else {
                    None
                };
                *portal = if shape.is_portal() {
                    let old = portal.clone().unwrap_or_default();
                    let new = match portal_names.get(&old) {
                        Some(n) => n.clone(),
                        None => {
                            let mut n = self.next_portal_name();
                            while portal_names.values().any(|v| *v == n) {
                                n.push('\'');
                            }
                            portal_names.insert(old, n.clone());
                            n
                        }
                    };
                    Some(new)
                } else {
                    None
                };
                *turntable = if shape.is_turntable() {
                    let id = self.next_id("TT");
                    let mut copy = turntable
                        .as_deref()
                        .and_then(|t| self.turntable(t))
                        .cloned()
                        .unwrap_or_default();
                    copy.id = id.clone();
                    copy.name = String::new();
                    copy.cw_event = None;
                    copy.ccw_event = None;
                    for tr in &mut copy.tracks {
                        tr.feedback_event = None;
                    }
                    self.turntables.push(copy);
                    Some(id)
                } else {
                    None
                };
                if block.as_deref().is_some_and(|b| self.block(b).is_none()) {
                    *block = None;
                }
                if block2.as_deref().is_some_and(|b| self.block(b).is_none())
                    || !shape.has_two_tracks()
                {
                    *block2 = None;
                }
            }
            placed.insert(el.pos());
            self.put(el);
        }
        Some(placed)
    }
}

/// Example layouts bundled with the app, as (title, JSON) pairs.
///
/// * **TMRC (from SCab2)**: the MIT Tech Model Railroad Club's layout,
///   converted from the track map of their SCab2 cab-control software
///   (`full.nlay`, MIT licensed) with `tools/scab2s4d.py`. Block and
///   turnout names match SCab2's; event IDs are left for you to fill in.
pub const EXAMPLES: &[(&str, &str)] =
    &[("TMRC (from SCab2)", include_str!("../layouts/tmrc.json"))];

/// The bundled example with this title (see [`EXAMPLES`]).
pub fn example(title: &str) -> Option<Layout> {
    EXAMPLES
        .iter()
        .find(|(t, _)| *t == title)
        .and_then(|(_, json)| Layout::from_json(json).ok())
}

/// The JSON Schema of the layout file format.
///
/// The committed copy is `docs/schema/layout-v1.schema.json`; a test fails
/// if it drifts from this output. Run
/// `cargo run -p s4d-core --example layout_schema --features schema > docs/schema/layout-v1.schema.json`
/// to regenerate it.
#[cfg(feature = "schema")]
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(Layout)
}

/// [`json_schema`] as pretty-printed JSON with a trailing newline, exactly
/// as committed.
///
/// # Panics
///
/// Never in practice: a schema always serialises.
#[cfg(feature = "schema")]
pub fn json_schema_string() -> String {
    let mut s = serde_json::to_string_pretty(&json_schema()).expect("schema serializes");
    s.push('\n');
    s
}

/// Schema stand-in for [`EventId`], which (de)serialises as a string.
#[cfg(feature = "schema")]
struct EventIdSchema;

#[cfg(feature = "schema")]
impl schemars::JsonSchema for EventIdSchema {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EventId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "An OpenLCB event ID: eight hex bytes, optionally separated by '.', ':', '-' or ' '.",
            "pattern": EVENT_ID_PATTERN,
            "examples": ["05.01.01.01.22.00.00.10"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(x: u16, y: u16, shape: Shape) -> Element {
        Element::Track {
            x,
            y,
            shape,
            rot: 0,
            block: None,
            turnout: None,
            turntable: None,
            turnout2: None,
            block2: None,
            portal: None,
            over: None,
        }
    }

    fn cells(v: &[(u16, u16)]) -> BTreeSet<(u16, u16)> {
        v.iter().copied().collect()
    }

    fn small() -> Layout {
        let mut l = Layout::empty("t");
        l.grid = Grid { cols: 4, rows: 3 };
        l
    }

    #[test]
    fn demo_layout_is_valid_and_round_trips() {
        let l = Layout::demo();
        assert!(l.validate().is_empty(), "{:?}", l.validate());
        assert_eq!(Layout::from_json(&l.to_json()).unwrap(), l);
        assert!(!l.blocks.is_empty() && !l.turnouts.is_empty());
    }

    #[test]
    fn examples_load_and_validate() {
        for (title, _) in EXAMPLES {
            let l = example(title).unwrap_or_else(|| panic!("{title} parses"));
            assert!(l.validate().is_empty(), "{title}: {:?}", l.validate());
            assert!(!l.turntables.is_empty());
        }
    }

    #[test]
    fn rotation() {
        let legs = Shape::TurnoutRight.legs().rotated(2);
        assert_eq!(legs.normal, vec![(0, 4)]);
        assert_eq!(legs.reverse, vec![(0, 5)]);
        assert_eq!(Shape::Bumper.legs().rotated(3).fixed, vec![(1, CENTER)]);
    }

    #[test]
    fn large_rotations_do_not_overflow() {
        for rot in [249u8, 250, 255] {
            let legs = Shape::Crossing.legs().rotated(rot);
            assert_eq!(legs, Shape::Crossing.legs().rotated(rot % 8));
        }
    }

    #[test]
    fn rot_is_normalised_on_load_and_flagged_in_code() {
        let l = Layout::from_json(
            r#"{"elements":[{"type":"track","x":0,"y":0,"shape":"straight","rot":255}]}"#,
        )
        .unwrap();
        assert!(matches!(l.elements[0], Element::Track { rot: 7, .. }));
        assert!(l.validate().is_empty());

        let mut l = Layout::empty("t");
        l.put(Element::Track {
            x: 0,
            y: 0,
            shape: Shape::Straight,
            rot: 9,
            block: None,
            turnout: None,
            turntable: None,
            turnout2: None,
            block2: None,
            portal: None,
            over: None,
        });
        assert_eq!(l.validate().len(), 1);
        assert!(l.validate()[0].0.contains("rotation 9"));
    }

    #[test]
    fn put_replaces_and_validate_flags() {
        let mut l = Layout::empty("t");
        l.put(Element::Track {
            x: 1,
            y: 1,
            shape: Shape::Straight,
            rot: 0,
            block: Some("B9".into()),
            turnout: None,
            turntable: None,
            turnout2: None,
            block2: None,
            portal: None,
            over: None,
        });
        l.put(track(1, 1, Shape::TurnoutLeft));
        assert_eq!(l.elements.len(), 1);
        assert_eq!(l.validate().len(), 1);
        assert_eq!(l.next_id("B"), "B1");
    }

    #[test]
    fn linking_is_symmetric() {
        let mut l = Layout::demo();
        l.set_link("T1", Some("T2"), false);
        assert_eq!(l.linked_partner("T1"), Some(("T2".into(), false)));
        assert_eq!(l.linked_partner("T2"), Some(("T1".into(), false)));
        // Re-linking T1 to T3 frees T2.
        l.set_link("T1", Some("T3"), true);
        assert_eq!(l.linked_partner("T2"), None);
        assert_eq!(l.linked_partner("T3"), Some(("T1".into(), true)));
        assert!(l.validate().is_empty());
        l.set_link("T3", None, false);
        assert_eq!(l.linked_partner("T1"), None);
        assert!(Layout::from_json(&l.to_json()).unwrap() == l);
    }

    #[test]
    fn validate_flags_asymmetric_inversion() {
        let mut l = Layout::demo();
        l.set_link("T1", Some("T2"), true);
        assert!(l.validate().is_empty());
        l.turnouts
            .iter_mut()
            .find(|t| t.id == "T2")
            .unwrap()
            .linked_inverted = false;
        let issues = l.validate();
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].0.contains("inverted"));
    }

    #[test]
    fn accepts_plan_style_json() {
        let l = Layout::from_json(
            r#"{"name":"x","grid":{"cols":4,"rows":4},
                "blocks":[{"id":"B1","occupied_event":"05.01.01.01.22.00.00.10"}],
                "turnouts":[{"id":"T1","normal_event":"05.01.01.01.22.00.01.00","reverse_event":"05.01.01.01.22.00.01.01"}],
                "elements":[{"type":"turnout","x":0,"y":0,"shape":"turnout_right","turnout":"T1","block":"B1"},
                            {"type":"label","x":1,"y":1,"text":"Hi"}]}"#,
        )
        .unwrap();
        assert!(l.validate().is_empty());
        assert!(Layout::from_json(r#"{"version":99}"#).is_err());
    }

    #[test]
    fn display_names() {
        let mut b = Block {
            id: "B1".into(),
            ..Default::default()
        };
        assert_eq!(b.display_name(), "B1");
        b.name = "Main".into();
        assert_eq!(b.display_name(), "Main");
        let mut t = Turnout {
            id: "T1".into(),
            ..Default::default()
        };
        assert_eq!(t.display_name(), "T1");
        t.name = "East".into();
        assert_eq!(t.display_name(), "East");
    }

    #[test]
    fn insert_row_moves_tiles_and_joins_track() {
        let mut l = Layout::empty("t");
        l.blocks.push(Block {
            id: "B".into(),
            ..Default::default()
        });
        // A diagonal from (2, 1) down-right to (3, 2), a N–S line at
        // column 6, and track in row 1 that doesn't cross.
        l.place_track(2, 1, Shape::Straight, 1, Some("B".into()));
        l.place_track(3, 2, Shape::Straight, 1, Some("B".into()));
        l.place_track(6, 1, Shape::Straight, 2, None);
        l.place_track(6, 2, Shape::Straight, 2, None);
        l.place_track(9, 1, Shape::Straight, 0, None);
        l.put(Element::Label {
            x: 0,
            y: 5,
            text: "L".into(),
        });
        let rows = l.grid.rows;
        assert_eq!(
            l.insert_row(2),
            Some(Inserted {
                joined: 2,
                gaps: vec![]
            })
        );
        assert_eq!(l.grid.rows, rows + 1);
        // Row 1 stays; row 2 and below moved down.
        assert!(l.element_at(2, 1).is_some() && l.element_at(9, 1).is_some());
        assert!(l.element_at(3, 3).is_some() && l.element_at(6, 3).is_some());
        assert!(l.element_at(0, 6).is_some());
        // The diagonal is joined by a curve from NW to S, in its block.
        match &l.elements[l.element_at(3, 2).unwrap()] {
            Element::Track { shape, block, .. } => {
                assert_eq!(*shape, Shape::Curve);
                assert_eq!(block.as_deref(), Some("B"));
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(l.connected_dirs(3, 2), vec![4, 7]);
        // ...and the diagonal below turned to come in from the N.
        match &l.elements[l.element_at(3, 3).unwrap()] {
            Element::Track { shape, rot, .. } => {
                let mut ends: Vec<u8> = shape
                    .legs()
                    .rotated(*rot)
                    .fixed
                    .iter()
                    .flat_map(|&(a, b)| [a, b])
                    .collect();
                ends.sort();
                assert_eq!((*shape, ends), (Shape::Curve, vec![0, 3]));
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(l.connected_dirs(6, 2), vec![0, 4]);
        assert!(l.element_at(9, 2).is_none());
        assert!(l.validate().is_empty(), "{:?}", l.validate());
    }

    #[test]
    fn insert_row_bends_the_track_above_a_switch() {
        let mut l = Layout::empty("t");
        l.place_track(2, 1, Shape::Straight, 1, None);
        l.place_track(3, 2, Shape::TurnoutLeft, 1, None);
        let before = l.elements[l.element_at(3, 2).unwrap()].clone();
        // The diagonal above bends instead of the switch below.
        assert_eq!(l.insert_row(2).unwrap().joined, 1);
        assert_eq!(l.connected_dirs(2, 1), vec![4]);
        assert_eq!(l.connected_dirs(2, 2), vec![0, 3]);
        let mut moved = before;
        moved.set_pos((3, 3));
        assert_eq!(l.elements[l.element_at(3, 3).unwrap()], moved);
    }

    #[test]
    fn insert_row_joins_a_switch_in_the_block_beyond_it() {
        let mut l = Layout::empty("t");
        l.place_track(2, 1, Shape::TurnoutRight, 0, Some("A".into()));
        l.place_track(3, 2, Shape::Straight, 1, Some("B".into()));
        assert_eq!(l.insert_row(2).unwrap().joined, 1);
        match &l.elements[l.element_at(3, 2).unwrap()] {
            Element::Track { block, .. } => assert_eq!(block.as_deref(), Some("B")),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn insert_row_reports_what_it_cant_join() {
        // Switch to switch on a diagonal: nothing can bend.
        let mut l = Layout::empty("t");
        l.place_track(2, 1, Shape::TurnoutRight, 7, None);
        l.place_track(1, 2, Shape::TurnoutRight, 7, None);
        assert!(l.connected_dirs(2, 1).contains(&5));
        let done = l.insert_row(2).unwrap();
        assert_eq!((done.joined, done.gaps), (0, vec![(2, 1)]));
        assert!(l.element_at(1, 3).is_some());
    }

    #[test]
    fn adopt_block_needs_the_same_block_at_both_ends() {
        let mut l = Layout::empty("t");
        for id in ["A", "B"] {
            l.blocks.push(Block {
                id: id.into(),
                ..Default::default()
            });
        }
        // A — . . — B: different blocks, nothing changes.
        l.place_track(1, 1, Shape::Straight, 0, Some("A".into()));
        l.place_track(2, 1, Shape::Straight, 0, None);
        l.place_track(3, 1, Shape::Straight, 0, None);
        l.place_track(4, 1, Shape::Straight, 0, Some("B".into()));
        assert_eq!(l.adopt_block(2, 1), None);
        // A — . / . — A, round a curve: both join A.
        l.place_track(1, 3, Shape::Straight, 0, Some("A".into()));
        l.place_track(2, 3, Shape::Curve, 0, None); // W to NE
        l.place_track(3, 2, Shape::Straight, 3, Some("A".into())); // SW–NE
        assert_eq!(l.adopt_block(2, 3).as_deref(), Some("A"));
        // Tiles that already have a block, and switches, are left alone.
        assert_eq!(l.adopt_block(1, 1), None);
        l.place_track(6, 5, Shape::TurnoutLeft, 0, None);
        assert_eq!(l.adopt_block(6, 5), None);
    }

    #[test]
    fn insert_column_joins_track_and_handles_edges() {
        let mut l = small();
        l.place_track(0, 1, Shape::Straight, 0, None);
        l.place_track(1, 1, Shape::Straight, 0, None);
        assert_eq!(l.insert_column(1).unwrap().joined, 1);
        assert_eq!(l.grid.cols, 5);
        assert_eq!(l.connected_dirs(1, 1), vec![2, 6]);
        assert!(l.element_at(2, 1).is_some());
        // At the very start or end nothing needs joining.
        assert_eq!(l.insert_column(0), Some(Inserted::default()));
        assert!(l.element_at(1, 1).is_some() && l.element_at(0, 1).is_none());
        assert_eq!(l.insert_column(6), Some(Inserted::default()));
        assert_eq!(l.grid.cols, 7);
        assert_eq!(l.insert_column(8), None);
        assert_eq!(l.grid.cols, 7);
    }

    #[test]
    fn move_cells_moves_and_ignores_empty() {
        let mut l = small();
        l.put(track(0, 0, Shape::Straight));
        l.put(Element::Label {
            x: 1,
            y: 0,
            text: "A".into(),
        });
        // (2,2) is empty and ignored.
        let moved = l
            .move_cells(&cells(&[(0, 0), (1, 0), (2, 2)]), 1, 1)
            .unwrap();
        assert_eq!(moved, cells(&[(1, 1), (2, 1)]));
        assert!(l.element_at(1, 1).is_some());
        assert!(
            matches!(&l.elements[l.element_at(2, 1).unwrap()], Element::Label { text, .. } if text == "A")
        );
        assert!(l.element_at(0, 0).is_none());
    }

    #[test]
    fn move_cells_into_itself_is_fine() {
        // Shifting a row of tiles along by one overlaps only selected tiles.
        let mut l = small();
        for x in 0..3 {
            l.put(track(x, 0, Shape::Straight));
        }
        let moved = l
            .move_cells(&cells(&[(0, 0), (1, 0), (2, 0)]), 1, 0)
            .unwrap();
        assert_eq!(moved, cells(&[(1, 0), (2, 0), (3, 0)]));
        assert_eq!(l.elements.len(), 3);
    }

    #[test]
    fn move_cells_refuses_edges_and_overlaps() {
        let mut l = small();
        l.put(track(0, 0, Shape::Straight));
        l.put(track(3, 2, Shape::Curve));
        let before = l.clone();
        let sel = cells(&[(0, 0)]);
        assert_eq!(
            l.move_cells(&sel, -1, 0),
            Err(MoveError::OutOfGrid { x: 0, y: 0 })
        );
        assert_eq!(
            l.move_cells(&sel, 0, 3),
            Err(MoveError::OutOfGrid { x: 0, y: 0 })
        );
        assert_eq!(
            l.move_cells(&sel, 3, 2),
            Err(MoveError::Overlap { x: 3, y: 2 })
        );
        assert_eq!(l, before);
        // The far corner is reachable when it's empty.
        assert_eq!(l.move_cells(&sel, 3, 1).unwrap(), cells(&[(3, 1)]));
        // An empty selection is a no-op.
        assert_eq!(l.move_cells(&cells(&[(0, 0)]), 5, 5).unwrap(), cells(&[]));
    }

    #[test]
    fn remove_rotate_assign() {
        let mut l = small();
        l.blocks.push(Block {
            id: "B1".into(),
            ..Default::default()
        });
        l.put(Element::Track {
            x: 0,
            y: 0,
            shape: Shape::Straight,
            rot: 7,
            block: None,
            turnout: None,
            turntable: None,
            turnout2: None,
            block2: None,
            portal: None,
            over: None,
        });
        l.put(Element::Label {
            x: 1,
            y: 0,
            text: "x".into(),
        });
        l.put(track(2, 0, Shape::Curve));
        let sel = cells(&[(0, 0), (1, 0)]);
        l.rotate_cells(&sel, 2);
        assert!(matches!(l.elements[0], Element::Track { rot: 1, .. }));
        assert!(matches!(l.elements[2], Element::Track { rot: 0, .. }));
        assert_eq!(l.assign_block(&sel, Some("B1")), 1);
        assert!(matches!(&l.elements[0], Element::Track { block: Some(b), .. } if b == "B1"));
        assert_eq!(l.assign_block(&sel, None), 1);
        assert!(matches!(&l.elements[0], Element::Track { block: None, .. }));
        assert_eq!(l.remove_cells(&sel), 2);
        assert_eq!(l.elements.len(), 1);
        assert_eq!(l.remove_cells(&sel), 0);
    }

    #[test]
    fn remove_block_and_turnout_clear_references() {
        let mut l = Layout::demo();
        l.set_link("T1", Some("T2"), false);
        assert!(l.remove_block("B1"));
        assert!(!l.remove_block("B1"));
        assert!(l.remove_turnout("T1"));
        assert!(!l.remove_turnout("T1"));
        assert_eq!(l.linked_partner("T2"), None);
        // Turnout tiles that lost their turnout are flagged; nothing dangles.
        assert!(l
            .validate()
            .iter()
            .all(|i| i.0.contains("has no turnout assigned")));
        assert!(l.elements.iter().all(|e| !matches!(e,
            Element::Track { block: Some(b), .. } if b == "B1")));
    }

    #[test]
    fn place_track_creates_and_keeps_turnouts() {
        let mut l = small();
        assert_eq!(l.place_track(0, 0, Shape::Straight, 0, None), None);
        let t = l.place_track(1, 0, Shape::TurnoutLeft, 9, None).unwrap();
        assert_eq!(t, "T1");
        assert!(l.turnout("T1").is_some());
        assert!(matches!(l.elements[1], Element::Track { rot: 1, .. }));
        // Re-placing a turnout shape keeps the id.
        assert_eq!(
            l.place_track(1, 0, Shape::Wye, 0, Some("B1".into())),
            Some("T1".into())
        );
        assert_eq!(l.turnouts.len(), 1);
        // A plain shape drops the reference (the definition stays).
        assert_eq!(l.place_track(1, 0, Shape::Straight, 0, None), None);
        assert!(matches!(
            &l.elements[1],
            Element::Track { turnout: None, .. }
        ));
        // Outside the grid: ignored.
        assert_eq!(l.place_track(4, 0, Shape::TurnoutLeft, 0, None), None);
        assert_eq!(l.elements.len(), 2);
        assert_eq!(l.turnouts.len(), 1);
    }

    #[test]
    fn set_tile_shape_follows_turnout_rule() {
        let mut l = small();
        l.place_track(0, 0, Shape::Straight, 3, Some("B1".into()));
        l.put(Element::Label {
            x: 1,
            y: 0,
            text: "x".into(),
        });
        assert_eq!(l.set_tile_shape(1, 0, Shape::Wye), None);
        assert_eq!(l.set_tile_shape(2, 2, Shape::Wye), None);
        let t = l.set_tile_shape(0, 0, Shape::TurnoutRight).unwrap();
        assert_eq!(l.set_tile_shape(0, 0, Shape::Wye), Some(t.clone()));
        assert!(matches!(&l.elements[0],
            Element::Track { shape: Shape::Wye, rot: 3, block: Some(_), turnout: Some(x), .. } if *x == t));
        assert_eq!(l.set_tile_shape(0, 0, Shape::Curve), None);
        assert_eq!(l.turnouts.len(), 1);
    }

    #[test]
    fn paste_finds_free_diagonal() {
        let mut l = small();
        l.blocks.push(Block {
            id: "B1".into(),
            ..Default::default()
        });
        l.place_track(0, 0, Shape::TurnoutLeft, 0, Some("B1".into()));
        // Occupies the offset-1 target of (0,0).
        l.place_track(1, 1, Shape::Straight, 0, None);
        let clip = [
            Element::Track {
                x: 0,
                y: 0,
                shape: Shape::TurnoutLeft,
                rot: 0,
                block: Some("B1".into()),
                turnout: Some("T1".into()),
                turntable: None,
                turnout2: None,
                block2: None,
                portal: None,
                over: None,
            },
            Element::Label {
                x: 1,
                y: 0,
                text: "L".into(),
            },
            Element::Track {
                x: 0,
                y: 0,
                shape: Shape::Straight,
                rot: 0,
                block: Some("gone".into()),
                turnout: Some("T1".into()),
                turntable: None,
                turnout2: None,
                block2: None,
                portal: None,
                over: None,
            },
        ];
        let placed = l.paste(&clip[..2]).unwrap();
        assert_eq!(placed, cells(&[(2, 2), (3, 2)]));
        let t = match &l.elements[l.element_at(2, 2).unwrap()] {
            Element::Track { turnout, block, .. } => {
                assert_eq!(block.as_deref(), Some("B1"));
                turnout.clone().unwrap()
            }
            _ => panic!(),
        };
        assert_ne!(t, "T1");
        assert!(l.turnout(&t).is_some());
        // Nothing fits any more for a 2-wide clip.
        let before = l.clone();
        assert_eq!(l.paste(&clip[..2]), None);
        assert_eq!(l, before);
        // Non-turnout tiles drop turnout refs and unknown blocks.
        let mut l = small();
        let placed = l.paste(&clip[2..]).unwrap();
        assert_eq!(placed, cells(&[(1, 1)]));
        assert!(matches!(
            &l.elements[0],
            Element::Track {
                block: None,
                turnout: None,
                ..
            }
        ));
        assert!(l.validate().is_empty());
        assert_eq!(l.paste(&[]), Some(cells(&[])));
    }

    #[test]
    fn turntable_placement_picks_up_connected_tracks() {
        let mut l = Layout::empty("t");
        l.place_track(4, 5, Shape::Straight, 0, None);
        l.place_track(5, 4, Shape::Straight, 0, None); // N of (5,5) but running E-W
        l.place_track(6, 5, Shape::Straight, 0, None);
        assert_eq!(l.place_track(5, 5, Shape::Turntable, 3, None), None);
        let Element::Track {
            turntable: Some(id),
            rot,
            ..
        } = &l.elements[3]
        else {
            panic!()
        };
        assert_eq!(*rot, 0);
        let id = id.clone();
        let angles: Vec<u16> = l
            .turntable(&id)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.angle)
            .collect();
        assert_eq!(angles, vec![90, 270]);
        // Turn the N track towards the pit and pick it up.
        l.rotate_cells(&cells(&[(5, 4)]), 2);
        assert_eq!(l.add_connected_leads(&id), 1);
        assert_eq!(l.add_connected_leads(&id), 0);
        assert!(l.validate().is_empty(), "{:?}", l.validate());
        l.place_track(5, 5, Shape::Turntable, 0, None);
        assert_eq!(l.turntables.len(), 1);
        l.set_tile_shape(5, 5, Shape::Straight);
        assert!(matches!(
            &l.elements[3],
            Element::Track {
                turntable: None,
                ..
            }
        ));
    }

    #[test]
    fn rotating_a_turntable_turns_its_tracks() {
        let mut l = Layout::empty("t");
        let id = l.place_roundhouse(5, 5, 1, 0, 1, 2, None).unwrap();
        l.rotate_cells(&cells(&[(5, 5)]), 2);
        let t = l.turntable(&id).unwrap();
        let named: Vec<(u16, &str)> = t
            .tracks
            .iter()
            .map(|t| (t.angle, t.name.as_str()))
            .collect();
        assert_eq!(named, vec![(0, "Lead"), (173, "Stall 1"), (188, "Stall 2")]);
        assert!(l.validate().is_empty());
    }

    #[test]
    fn turntable_track_order_and_steps() {
        let mut t = Turntable::default();
        assert_eq!(t.free_stall_angle(), 90);
        assert_eq!(t.add_track(370, "a", false), Some(0));
        assert_eq!(t.add_track(10, "dup", false), None);
        assert_eq!(t.free_stall_angle(), 190);
        t.add_track(200, "b", true);
        t.add_track(100, "c", true);
        let names: Vec<&str> = t.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c", "b"]);
        assert_eq!(t.step_from(0, -1), Some(2));
        assert_eq!(t.step_from(2, 1), Some(0));
        assert_eq!(t.tracks[0].dir(), None);
        assert_eq!(t.tracks[0].display_name(), "a");
        t.tracks[0].name.clear();
        assert_eq!(t.tracks[0].display_name(), "10°");
    }

    #[test]
    fn place_roundhouse_refuses_without_changes() {
        let mut l = small(); // 4 x 3
        let before = l.clone();
        assert_eq!(
            l.place_roundhouse(1, 1, 1, 0, 2, 3, None),
            Err(MoveError::OutOfGrid { x: 0, y: 1 })
        );
        l.place_track(2, 1, Shape::Straight, 0, None);
        let before2 = l.clone();
        assert_eq!(
            l.place_roundhouse(3, 1, 1, 0, 1, 3, None),
            Err(MoveError::Overlap { x: 2, y: 1 })
        );
        assert_eq!(l, before2);
        assert_ne!(l, before);
        // Rotated 180°: the lead goes E.
        let mut l = small();
        let id = l
            .place_roundhouse(1, 1, 1, 4, 2, 3, Some("B1".into()))
            .unwrap();
        let t = l.turntable(&id).unwrap();
        assert_eq!(t.tracks.iter().find(|t| !t.stall).unwrap().angle, 90);
        let stalls: Vec<u16> = t
            .tracks
            .iter()
            .filter(|t| t.stall)
            .map(|t| t.angle)
            .collect();
        assert_eq!(stalls, vec![255, 270, 285]);
        assert!(matches!(
            &l.elements[l.element_at(3, 1).unwrap()],
            Element::Track {
                shape: Shape::Straight,
                rot: 0,
                ..
            }
        ));
        // A bigger pit starts the lead further out.
        let mut l = Layout::empty("t");
        l.place_roundhouse(10, 5, 3, 0, 1, 0, None).unwrap();
        assert!(l.element_at(9, 5).is_none());
        assert!(l.element_at(8, 5).is_some());
    }

    #[test]
    fn turntables_paste_remove_and_validate() {
        let mut l = Layout::demo();
        let tt: Vec<Element> = l
            .elements
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Element::Track {
                        shape: Shape::Turntable,
                        ..
                    }
                )
            })
            .cloned()
            .collect();
        let placed = l.paste(&tt).unwrap();
        let (x, y) = *placed.iter().next().unwrap();
        let Element::Track {
            turntable: Some(id),
            ..
        } = &l.elements[l.element_at(x, y).unwrap()]
        else {
            panic!()
        };
        let copy = l.turntable(id).unwrap();
        assert_ne!(copy.id, "TT1");
        assert_eq!(copy.tracks.len(), 8);
        assert!(copy.cw_event.is_none() && copy.tracks.iter().all(|t| t.feedback_event.is_none()));
        assert!(l.validate().is_empty());

        assert!(l.remove_turntable("TT1"));
        assert!(!l.remove_turntable("TT1"));
        assert_eq!(l.validate().len(), 1);
        assert!(l.validate()[0].0.contains("has no turntable assigned"));

        let mut l = Layout::demo();
        l.turntables[0].tracks[1].angle = l.turntables[0].tracks[0].angle;
        l.turntables[0].tracks[2].angle = 400;
        let issues = l.validate();
        assert_eq!(issues.len(), 3, "{issues:?}");
        assert!(issues[0].0.contains("two tracks at 75°"));
        assert!(issues[1].0.contains("at 400°"));
        assert!(issues[2].0.contains("clockwise order"));
        assert_eq!(l.next_id("TT"), "TT2");
    }

    #[test]
    fn three_way_tiles_get_two_turnouts() {
        let mut l = small();
        let t = l.place_track(1, 1, Shape::ThreeWay, 0, None).unwrap();
        let Element::Track {
            turnout2: Some(t2), ..
        } = &l.elements[0]
        else {
            panic!()
        };
        let t2 = t2.clone();
        assert_ne!(t, t2);
        assert_eq!(l.turnouts.len(), 2);
        // Re-placing keeps both; another turnout shape drops the second.
        l.place_track(1, 1, Shape::ThreeWay, 2, None);
        assert_eq!(l.turnouts.len(), 2);
        assert!(l.validate().is_empty());
        l.set_tile_shape(1, 1, Shape::TurnoutLeft);
        assert!(matches!(
            &l.elements[0],
            Element::Track { turnout2: None, .. }
        ));
        l.set_tile_shape(1, 1, Shape::ThreeWay);
        assert!(l.remove_turnout(&t2) || l.turnouts.len() == 3);
        let mut l = small();
        l.place_track(1, 1, Shape::ThreeWay, 0, None);
        let Element::Track {
            turnout2: Some(t2), ..
        } = l.elements[0].clone()
        else {
            panic!()
        };
        assert!(l.remove_turnout(&t2));
        assert!(l.validate()[0].0.contains("no right-hand turnout"));
        // Slips get two turnouts too.
        let mut l = small();
        l.place_track(2, 1, Shape::DoubleSlip, 1, None);
        assert_eq!(l.turnouts.len(), 2);
        assert!(l.validate().is_empty());
        assert!(Shape::DoubleSlip.is_slip_road((6, 3)));
        assert!(!Shape::DoubleSlip.is_slip_road((6, 2)));
        assert!(!Shape::ThreeWay.is_slip_road((6, 3)));
    }

    #[test]
    fn crossings_have_a_second_block() {
        let mut l = small();
        for id in ["B1", "B2"] {
            l.blocks.push(Block {
                id: id.into(),
                ..Default::default()
            });
        }
        l.place_track(1, 1, Shape::CrossingDiag, 0, Some("B1".into()));
        if let Element::Track { block2, .. } = &mut l.elements[0] {
            *block2 = Some("B2".into());
        }
        assert!(l.validate().is_empty());
        // Re-placing a crossing keeps it; a plain shape drops it.
        l.place_track(1, 1, Shape::DoubleSlip, 0, Some("B1".into()));
        assert!(matches!(&l.elements[0], Element::Track { block2: Some(b), .. } if b == "B2"));
        assert!(l.remove_block("B2"));
        assert!(matches!(
            &l.elements[0],
            Element::Track { block2: None, .. }
        ));
        if let Element::Track { block2, .. } = &mut l.elements[0] {
            *block2 = Some("B9".into());
        }
        assert_eq!(l.validate()[0].0, "tile at 1,1 refers to unknown block B9");
        l.set_tile_shape(1, 1, Shape::Straight);
        assert!(matches!(
            &l.elements[0],
            Element::Track { block2: None, .. }
        ));
    }

    #[test]
    fn routes_join_free_ends_and_avoid_track() {
        let mut l = Layout::empty("t");
        l.grid = Grid { cols: 20, rows: 10 };
        // A wall of track between the two ends forces a detour.
        for y in 0..8 {
            l.place_track(10, y, Shape::Straight, 2, None);
        }
        l.place_track(3, 2, Shape::Bumper, 4, None); // open to the E
        l.place_track(16, 2, Shape::Bumper, 0, None); // open to the W
        let before = l.clone();
        let tiles = l.route_track((3, 2), (16, 2), Some("B1".into())).unwrap();
        assert_eq!(l, before);
        assert!(tiles
            .iter()
            .all(|t| t.pos().1 >= 2 && l.element_at(t.pos().0, t.pos().1).is_none()));
        assert!(tiles.iter().any(|t| t.pos().1 == 9 || t.pos().1 == 8));
        let cells = l.draw_route((3, 2), (16, 2), None).unwrap();
        assert_eq!(cells.len(), tiles.len());
        // Every new tile is joined at both ends, and so are the old ends.
        for &(x, y) in &cells {
            assert!(l.free_ends(x, y).is_empty(), "loose end at {x},{y}");
        }
        assert!(l.free_ends(3, 2).is_empty() && l.free_ends(16, 2).is_empty());
        // Empty endpoints get tiles of their own.
        let mut l = Layout::empty("t");
        let tiles = l.route_track((1, 1), (5, 1), None).unwrap();
        assert_eq!(tiles.len(), 5);
        // Errors.
        assert_eq!(
            l.route_track((1, 1), (1, 1), None),
            Err(RouteError::SameCell)
        );
        assert_eq!(
            l.route_track((1, 1), (99, 1), None),
            Err(RouteError::OutsideGrid)
        );
        l.place_track(1, 1, Shape::Straight, 0, None);
        l.place_track(0, 1, Shape::Bumper, 4, None);
        l.place_track(2, 1, Shape::Bumper, 0, None);
        assert_eq!(
            l.route_track((1, 1), (8, 8), None),
            Err(RouteError::NoFreeEnd { x: 1, y: 1 })
        );
        let mut l = small(); // 4 x 3: box an end in
        l.place_track(0, 1, Shape::Bumper, 4, None);
        l.place_track(1, 0, Shape::Straight, 0, None);
        l.place_track(1, 1, Shape::Straight, 2, None);
        l.place_track(1, 2, Shape::Straight, 0, None);
        assert_eq!(
            l.route_track((0, 1), (3, 1), None),
            Err(RouteError::NoRoute)
        );
    }

    #[test]
    fn exits_to_shapes_round_trip() {
        for a in 0..8u8 {
            for turn in [3u8, 4, 5] {
                let b = (a + turn) % 8;
                let (shape, rot) = shape_for_exits(a, b);
                let legs = shape.legs().rotated(rot);
                let (p, q) = legs.fixed[0];
                assert!(
                    (p, q) == (a, b) || (p, q) == (b, a),
                    "{a}-{b} gave {shape:?} rot {rot}: {p}-{q}"
                );
            }
        }
    }

    #[test]
    fn portals_pair_up() {
        let mut l = Layout::empty("t");
        l.place_track(0, 2, Shape::Portal, 4, None);
        assert_eq!(l.validate()[0].0, "portal A has no other end");
        l.place_track(31, 2, Shape::Portal, 0, None);
        assert!(l.validate().is_empty());
        assert_eq!(l.portal_partner(31, 2), Some((0, 2)));
        assert_eq!(l.portal_partner(5, 5), None);
        // The next pair is B; a third end of A is flagged.
        l.place_track(0, 4, Shape::Portal, 4, None);
        assert!(matches!(&l.elements[2], Element::Track { portal: Some(p), .. } if p == "B"));
        l.put(Element::Track {
            x: 3,
            y: 3,
            shape: Shape::Portal,
            rot: 0,
            block: None,
            turnout: None,
            turnout2: None,
            block2: None,
            turntable: None,
            portal: Some("A".into()),
            over: None,
        });
        let issues = l.validate();
        assert!(issues
            .iter()
            .any(|i| i.0 == "portal A has 3 ends (it needs exactly 2)"));
        assert!(issues.iter().any(|i| i.0 == "portal B has no other end"));
        // Pasting a pair gives it a fresh name, still paired.
        let mut l = Layout::empty("t");
        l.place_track(0, 2, Shape::Portal, 4, None);
        l.place_track(4, 2, Shape::Portal, 0, None);
        let clip = l.elements.clone();
        l.paste(&clip).unwrap();
        assert!(l.validate().is_empty(), "{:?}", l.validate());
        assert_eq!(l.next_portal_name(), "C");
        let mut l = Layout::empty("t");
        for i in 0..27 {
            l.put(Element::Track {
                x: i,
                y: 0,
                shape: Shape::Portal,
                rot: 0,
                block: None,
                turnout: None,
                turnout2: None,
                block2: None,
                turntable: None,
                portal: Some(l.next_portal_name()),
                over: None,
            });
        }
        assert_eq!(l.next_portal_name(), "AB");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn schema_matches_committed_file() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/schema/layout-v1.schema.json"
        );
        let generated = json_schema_string();
        if std::env::var_os("S4D_BLESS").is_some() {
            std::fs::write(path, &generated).unwrap();
        }
        let committed = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            committed == generated,
            "docs/schema/layout-v1.schema.json is out of date; regenerate with \
             `S4D_BLESS=1 cargo test -p s4d-core --features schema schema_matches`"
        );
    }

    #[cfg(feature = "schema")]
    #[test]
    fn schema_accepts_demo_shape() {
        let s = json_schema_string();
        assert!(s.contains(EVENT_ID_PATTERN.replace('\\', "\\\\").as_str()));
        assert!(s.contains("\"maximum\": 7"));
        // The pattern accepts every way rolcb parses event IDs.
        for ok in [
            "05.01.01.01.22.00.00.10",
            "0501010122000010",
            "05:01:01:01:22:00:00:10",
        ] {
            assert!(ok.parse::<EventId>().is_ok());
        }
    }
}
