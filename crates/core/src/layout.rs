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
        /// The turnout this tile shows, by id (turnout shapes only).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turnout: Option<String>,
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

/// Tile shapes. Each can be rotated in 45° steps (`rot` 0..=7, clockwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// W to E.
    Straight,
    /// W to NE (a 45° curve).
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
}

impl Shape {
    /// Every shape, in palette order.
    pub const ALL: [Shape; 8] = [
        Shape::Straight,
        Shape::Curve,
        Shape::Bumper,
        Shape::Crossing,
        Shape::CrossingDiag,
        Shape::TurnoutLeft,
        Shape::TurnoutRight,
        Shape::Wye,
    ];

    /// True for shapes that need a [`Turnout`].
    pub fn is_turnout(self) -> bool {
        matches!(self, Shape::TurnoutLeft | Shape::TurnoutRight | Shape::Wye)
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

impl Layout {
    /// A layout with no tiles, blocks or turnouts and the default grid.
    pub fn empty(name: &str) -> Layout {
        Layout {
            version: FORMAT_VERSION,
            name: name.to_string(),
            grid: Grid::default(),
            blocks: Vec::new(),
            turnouts: Vec::new(),
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

    /// Next free id like "B7" / "T3".
    ///
    /// # Panics
    ///
    /// Never in practice (it would need billions of ids in use).
    pub fn next_id(&self, prefix: &str) -> String {
        let used = |id: &str| {
            self.blocks.iter().any(|b| b.id == id) || self.turnouts.iter().any(|t| t.id == id)
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
    ///     block: Some("B9".into()), turnout: None,
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
        for el in &self.elements {
            let (x, y) = el.pos();
            if x >= self.grid.cols || y >= self.grid.rows {
                out.push(Issue(format!("tile at {x},{y} is outside the grid")));
            }
            if let Element::Track {
                block,
                turnout,
                shape,
                rot,
                ..
            } = el
            {
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
            }
        }
        out
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

    /// Remove every tile in `cells`. Block and turnout definitions are kept.
    /// Returns how many tiles were removed.
    pub fn remove_cells(&mut self, cells: &BTreeSet<(u16, u16)>) -> usize {
        let before = self.elements.len();
        self.elements.retain(|e| !cells.contains(&e.pos()));
        before - self.elements.len()
    }

    /// Rotate each track tile in `cells` clockwise by `delta` × 45° about its
    /// own centre (labels are unaffected). Use `delta = 7` for one step
    /// anticlockwise.
    pub fn rotate_cells(&mut self, cells: &BTreeSet<(u16, u16)>, delta: u8) {
        for el in &mut self.elements {
            if !cells.contains(&el.pos()) {
                continue;
            }
            if let Element::Track { rot, .. } = el {
                *rot = (*rot % 8 + delta % 8) % 8;
            }
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
            if let Element::Track { block, .. } = el {
                if block.as_deref() == Some(id) {
                    *block = None;
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
            if let Element::Track { turnout, .. } = el {
                if turnout.as_deref() == Some(id) {
                    *turnout = None;
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
    /// shapes return `None`. Cells outside the grid are ignored (nothing is
    /// placed, `None` is returned). `rot` is taken modulo 8.
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
        self.put(Element::Track {
            x,
            y,
            shape,
            rot: rot % 8,
            block,
            turnout: turnout.clone(),
        });
        turnout
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
        if let Element::Track {
            shape: s, turnout, ..
        } = &mut self.elements[i]
        {
            *s = shape;
            *turnout = id.clone();
        }
        id
    }

    /// Paste copied tiles, shifted diagonally by the first offset `k` in
    /// `1..=16` (both x and y move by `k`) at which every tile lands inside
    /// the grid on an empty cell. Turnout tiles get new blank turnouts;
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
        let mut placed = BTreeSet::new();
        for t in tiles {
            let mut el = t.clone();
            let (x, y) = el.pos();
            el.set_pos((x + k, y + k));
            if let Element::Track {
                shape,
                block,
                turnout,
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
                if block.as_deref().is_some_and(|b| self.block(b).is_none()) {
                    *block = None;
                }
            }
            placed.insert(el.pos());
            self.put(el);
        }
        Some(placed)
    }
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
