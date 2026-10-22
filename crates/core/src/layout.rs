//! The dispatch panel layout file: a grid of track tiles, plus the blocks and
//! turnouts they belong to and the OpenLCB events that drive them.

use rolcb::EventId;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub grid: Grid,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub turnouts: Vec<Turnout>,
    #[serde(default)]
    pub elements: Vec<Element>,
}

fn default_version() -> u32 {
    FORMAT_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
}

impl Default for Grid {
    fn default() -> Self {
        Grid { cols: 32, rows: 16 }
    }
}

/// A detection block. Occupancy detectors usually produce one event when the
/// block becomes occupied and another when it clears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupied_event: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_event: Option<EventId>,
}

/// A turnout. Sending `normal_event` / `reverse_event` throws it; optional
/// feedback events report the real point position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turnout {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_event: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_event: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<Feedback>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Feedback {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_event: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_event: Option<EventId>,
}

impl Turnout {
    pub fn has_feedback(&self) -> bool {
        self.feedback
            .as_ref()
            .is_some_and(|f| f.normal_event.is_some() || f.reverse_event.is_some())
    }
}

/// Something drawn on the grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    #[serde(alias = "turnout")]
    Track {
        x: u16,
        y: u16,
        shape: Shape,
        #[serde(default)]
        rot: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turnout: Option<String>,
    },
    Label {
        x: u16,
        y: u16,
        text: String,
    },
}

impl Element {
    pub fn pos(&self) -> (u16, u16) {
        match self {
            Element::Track { x, y, .. } | Element::Label { x, y, .. } => (*x, *y),
        }
    }
}

/// Tile shapes. Each can be rotated in 45° steps (`rot` 0..=7, clockwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Straight,
    Curve,
    Bumper,
    Crossing,
    CrossingDiag,
    TurnoutLeft,
    TurnoutRight,
    Wye,
}

impl Shape {
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

    pub fn is_turnout(self) -> bool {
        matches!(self, Shape::TurnoutLeft | Shape::TurnoutRight | Shape::Wye)
    }

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

pub const CENTER: u8 = 8;

pub type Seg = (u8, u8);

/// The track segments of a tile. Turnouts split into normal / reverse legs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Legs {
    pub fixed: Vec<Seg>,
    pub normal: Vec<Seg>,
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

    /// Rotate clockwise by `rot` × 45°.
    pub fn rotated(&self, rot: u8) -> Legs {
        let r = |d: u8| if d == CENTER { d } else { (d + rot) % 8 };
        let map = |v: &Vec<Seg>| v.iter().map(|&(a, b)| (r(a), r(b))).collect();
        Legs {
            fixed: map(&self.fixed),
            normal: map(&self.normal),
            reverse: map(&self.reverse),
        }
    }
}

/// Offset from the tile centre (in tile units) of the edge/corner for `dir`.
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

/// Problems found by [`Layout::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue(pub String);

impl Layout {
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

    pub fn from_json(s: &str) -> Result<Layout, String> {
        let l: Layout = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if l.version > FORMAT_VERSION {
            return Err(format!(
                "layout format version {} is newer than this app supports ({FORMAT_VERSION})",
                l.version
            ));
        }
        Ok(l)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("layout serializes")
    }

    /// The built-in demo layout.
    pub fn demo() -> Layout {
        Layout::from_json(include_str!("../../../examples/demo-layout.json"))
            .expect("demo layout parses")
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn turnout(&self, id: &str) -> Option<&Turnout> {
        self.turnouts.iter().find(|t| t.id == id)
    }

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

    pub fn remove_at(&mut self, x: u16, y: u16) {
        self.elements.retain(|e| e.pos() != (x, y));
    }

    /// Next free id like "B7" / "T3".
    pub fn next_id(&self, prefix: &str) -> String {
        let used = |id: &str| {
            self.blocks.iter().any(|b| b.id == id) || self.turnouts.iter().any(|t| t.id == id)
        };
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| !used(id))
            .unwrap()
    }

    /// Report dangling references, duplicate ids and out-of-grid tiles.
    pub fn validate(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
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
        for el in &self.elements {
            let (x, y) = el.pos();
            if x >= self.grid.cols || y >= self.grid.rows {
                out.push(Issue(format!("tile at {x},{y} is outside the grid")));
            }
            if let Element::Track {
                block,
                turnout,
                shape,
                ..
            } = el
            {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        l.put(Element::Track {
            x: 1,
            y: 1,
            shape: Shape::TurnoutLeft,
            rot: 0,
            block: None,
            turnout: None,
        });
        assert_eq!(l.elements.len(), 1);
        assert_eq!(l.validate().len(), 1);
        assert_eq!(l.next_id("B"), "B1");
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
}
