//! Configuration Description Information (CDI): the XML document a node
//! serves (memory space `0xFF`) that describes its configuration memory.
//!
//! [`Cdi::parse`] turns it into a tree of [`Segment`]s, [`Group`]s and
//! [`Field`]s with every field's memory space and address worked out, so a
//! UI can read and write values with [`Field::decode`] and [`Field::encode`]
//! without knowing the layout rules.
//!
//! # Address rules (*CDI Standard*)
//!
//! Within a `<segment space="…" origin="…">`, a cursor starts at `origin`.
//! Each element first moves the cursor by its `offset` attribute (default
//! 0, may be negative), then occupies its size: `int` `size` bytes (default
//! 1), `string` and `blob` `size` bytes, `eventid` 8, `float` `size`
//! (default 4), `action` `size` (default 1). A `<group>` occupies the sum of
//! its contents, `replication` times over (default 1). Replicated groups are
//! expanded here into one [`Group`] per copy, named from `<repname>`.
//!
//! An `<acdi/>` element promises the standard user-info space `0xFB`
//! (version byte, 63-byte name, 64-byte description). If the CDI doesn't
//! describe that space itself, a "User info" segment is added for it.
//!
//! ```
//! use s4d_core::cdi::{Cdi, FieldKind};
//!
//! let cdi = Cdi::parse(r#"<cdi>
//!   <segment space="253">
//!     <group replication="2"><repname>Input</repname>
//!       <int size="2"><name>Debounce</name></int>
//!       <eventid><name>Active</name></eventid>
//!     </group>
//!   </segment>
//! </cdi>"#).unwrap();
//! let inputs = &cdi.segments[0].items;
//! let second = inputs[1].as_group().unwrap();
//! assert_eq!(second.name, "Input 2");
//! let active = second.items[1].as_field().unwrap();
//! assert_eq!((active.space, active.address, active.size), (253, 12, 8));
//! assert_eq!(active.kind, FieldKind::EventId);
//! ```

use rolcb::{float16, EventId};

/// The user-editable ACDI memory space (node name and description).
pub const SPACE_ACDI_USER: u8 = 0xFB;

/// A parsed CDI document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cdi {
    /// The `<identification>` block, if any.
    pub identification: Identification,
    /// Configuration segments, in document order.
    pub segments: Vec<Segment>,
}

/// What the node says it is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Identification {
    /// Manufacturer name.
    pub manufacturer: String,
    /// Model name.
    pub model: String,
    /// Hardware version.
    pub hardware_version: String,
    /// Software version.
    pub software_version: String,
}

/// One `<segment>`: a run of configuration in one memory space.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Display name (may be empty).
    pub name: String,
    /// Longer description (may be empty).
    pub description: String,
    /// Memory space number, e.g. `0xFD` for configuration.
    pub space: u8,
    /// Contents.
    pub items: Vec<Item>,
}

/// An entry in a segment or group.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A group of further items.
    Group(Group),
    /// A single value.
    Field(Field),
}

impl Item {
    /// The group, if this is one.
    pub fn as_group(&self) -> Option<&Group> {
        match self {
            Item::Group(g) => Some(g),
            Item::Field(_) => None,
        }
    }

    /// The field, if this is one.
    pub fn as_field(&self) -> Option<&Field> {
        match self {
            Item::Field(f) => Some(f),
            Item::Group(_) => None,
        }
    }
}

/// A `<group>` (or one copy of a replicated group).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Display name; for replicated groups `"<repname> <n>"`.
    pub name: String,
    /// Longer description (may be empty).
    pub description: String,
    /// Contents.
    pub items: Vec<Item>,
}

/// A single configuration value and where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Display name (may be empty).
    pub name: String,
    /// Longer description (may be empty).
    pub description: String,
    /// Memory space.
    pub space: u8,
    /// Address of the first byte.
    pub address: u32,
    /// Size in bytes.
    pub size: u32,
    /// What kind of value it holds.
    pub kind: FieldKind,
}

/// The type of a [`Field`].
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// Unsigned big-endian integer, optionally with named values.
    Int {
        /// Smallest allowed value.
        min: Option<i64>,
        /// Largest allowed value.
        max: Option<i64>,
        /// Factory default.
        default: Option<i64>,
        /// Named values (`<map>`), in document order.
        map: Vec<(i64, String)>,
    },
    /// NUL-terminated text of at most `size - 1` bytes.
    String,
    /// An 8-byte event ID.
    EventId,
    /// IEEE float of 2, 4 or 8 bytes.
    Float {
        /// Smallest allowed value.
        min: Option<f64>,
        /// Largest allowed value.
        max: Option<f64>,
        /// Factory default.
        default: Option<f64>,
    },
    /// A button that writes `value` when pressed.
    Action {
        /// Button label.
        button: String,
        /// Value written.
        value: i64,
    },
    /// Opaque bytes; shown but not edited.
    Blob,
}

impl Field {
    /// The value as text, from `bytes` read at [`address`](Field::address)
    /// (at least [`size`](Field::size) long).
    pub fn decode(&self, bytes: &[u8]) -> Option<String> {
        let b = bytes.get(..self.size as usize)?;
        Some(match &self.kind {
            FieldKind::Int { .. } | FieldKind::Action { .. } => {
                b.iter().fold(0u64, |a, x| a << 8 | *x as u64).to_string()
            }
            FieldKind::String => {
                let end = b.iter().position(|x| *x == 0).unwrap_or(b.len());
                String::from_utf8_lossy(&b[..end]).into_owned()
            }
            FieldKind::EventId => EventId::from_bytes(b).ok()?.to_string(),
            FieldKind::Float { .. } => match b.len() {
                2 => float16::to_f32(u16::from_be_bytes([b[0], b[1]])).to_string(),
                4 => f32::from_be_bytes(b.try_into().ok()?).to_string(),
                8 => f64::from_be_bytes(b.try_into().ok()?).to_string(),
                _ => return None,
            },
            FieldKind::Blob => b
                .iter()
                .map(|x| format!("{x:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
        })
    }

    /// The bytes to write for `text`, checked against the field's limits.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, String> {
        let text = text.trim_end_matches(['\r', '\n']);
        let size = self.size as usize;
        match &self.kind {
            FieldKind::Int { min, max, .. } => {
                let v: i64 = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("{:?} is not a whole number", text.trim()))?;
                if let Some(m) = min.filter(|m| v < *m) {
                    return Err(format!("must be at least {m}"));
                }
                if let Some(m) = max.filter(|m| v > *m) {
                    return Err(format!("must be at most {m}"));
                }
                let limit = if size >= 8 {
                    u64::MAX
                } else {
                    (1u64 << (size * 8)) - 1
                };
                if v < 0 || v as u64 > limit {
                    return Err(format!("must be between 0 and {limit}"));
                }
                Ok((v as u64).to_be_bytes()[8 - size.min(8)..].to_vec())
            }
            FieldKind::Action { value, .. } => {
                Ok((*value as u64).to_be_bytes()[8 - size.min(8)..].to_vec())
            }
            FieldKind::String => {
                if text.len() >= size {
                    return Err(format!("at most {} characters", size.saturating_sub(1)));
                }
                let mut b = text.as_bytes().to_vec();
                b.push(0);
                Ok(b)
            }
            FieldKind::EventId => text
                .trim()
                .parse::<EventId>()
                .map(|e| e.to_bytes().to_vec())
                .map_err(|_| "event IDs are 8 hex bytes, e.g. 05.01.01.01.22.00.00.FF".into()),
            FieldKind::Float { min, max, .. } => {
                let v: f64 = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("{:?} is not a number", text.trim()))?;
                if let Some(m) = min.filter(|m| v < *m) {
                    return Err(format!("must be at least {m}"));
                }
                if let Some(m) = max.filter(|m| v > *m) {
                    return Err(format!("must be at most {m}"));
                }
                Ok(match size {
                    2 => float16::from_f32(v as f32).to_be_bytes().to_vec(),
                    8 => v.to_be_bytes().to_vec(),
                    _ => (v as f32).to_be_bytes().to_vec(),
                })
            }
            FieldKind::Blob => Err("this value can't be edited".into()),
        }
    }
}

impl Cdi {
    /// Parse a CDI document. Unknown elements are skipped.
    pub fn parse(xml: &str) -> Result<Cdi, String> {
        // Nodes pad their CDI with NULs, and some put junk after it.
        let xml = xml.split('\0').next().unwrap_or("");
        let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        if root.tag_name().name() != "cdi" {
            return Err(format!(
                "expected <cdi>, found <{}>",
                root.tag_name().name()
            ));
        }
        let mut cdi = Cdi::default();
        let mut acdi = false;
        for el in root.children().filter(|n| n.is_element()) {
            match el.tag_name().name() {
                "identification" => {
                    let t = |name| child_text(el, name);
                    cdi.identification = Identification {
                        manufacturer: t("manufacturer"),
                        model: t("model"),
                        hardware_version: t("hardwareVersion"),
                        software_version: t("softwareVersion"),
                    };
                }
                "acdi" => acdi = true,
                "segment" => {
                    let space =
                        attr_num(el, "space")?.ok_or("<segment> without a space attribute")?;
                    let space = u8::try_from(space).map_err(|_| format!("bad space {space}"))?;
                    let origin = attr_num(el, "origin")?.unwrap_or(0);
                    let mut cursor = origin;
                    let items = parse_items(el, space, &mut cursor)?;
                    cdi.segments.push(Segment {
                        name: child_text(el, "name"),
                        description: child_text(el, "description"),
                        space,
                        items,
                    });
                }
                _ => {}
            }
        }
        if acdi && !cdi.segments.iter().any(|s| s.space == SPACE_ACDI_USER) {
            cdi.segments.insert(0, acdi_user_segment());
        }
        Ok(cdi)
    }

    /// Every field, depth first.
    pub fn fields(&self) -> Vec<&Field> {
        fn walk<'a>(items: &'a [Item], out: &mut Vec<&'a Field>) {
            for i in items {
                match i {
                    Item::Field(f) => out.push(f),
                    Item::Group(g) => walk(&g.items, out),
                }
            }
        }
        let mut out = Vec::new();
        for s in &self.segments {
            walk(&s.items, &mut out);
        }
        out
    }
}

/// The standard ACDI user space: name and description.
fn acdi_user_segment() -> Segment {
    let field = |name: &str, address, size| {
        Item::Field(Field {
            name: name.into(),
            description: String::new(),
            space: SPACE_ACDI_USER,
            address,
            size,
            kind: FieldKind::String,
        })
    };
    Segment {
        name: "User info".into(),
        description: String::new(),
        space: SPACE_ACDI_USER,
        items: vec![field("Name", 1, 63), field("Description", 64, 64)],
    }
}

fn child_text(el: roxmltree::Node, name: &str) -> String {
    el.children()
        .find(|c| c.has_tag_name(name))
        .and_then(|c| c.text())
        .map(|t| t.trim().to_string())
        .unwrap_or_default()
}

fn child_num<T: std::str::FromStr>(el: roxmltree::Node, name: &str) -> Option<T> {
    el.children()
        .find(|c| c.has_tag_name(name))
        .and_then(|c| c.text())
        .and_then(|t| t.trim().parse().ok())
}

/// A numeric attribute (decimal, or hex with `0x`).
fn attr_num(el: roxmltree::Node, name: &str) -> Result<Option<i64>, String> {
    let Some(v) = el.attribute(name) else {
        return Ok(None);
    };
    let v = v.trim();
    let n = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(h) => i64::from_str_radix(h, 16),
        None => v.parse(),
    };
    n.map(Some)
        .map_err(|_| format!("<{} {name}=\"{v}\"> is not a number", el.tag_name().name()))
}

fn parse_items(parent: roxmltree::Node, space: u8, cursor: &mut i64) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for el in parent.children().filter(|n| n.is_element()) {
        let tag = el.tag_name().name();
        let size_of =
            |default: i64| -> Result<i64, String> { Ok(attr_num(el, "size")?.unwrap_or(default)) };
        let (size, kind) = match tag {
            "group" => {
                *cursor += attr_num(el, "offset")?.unwrap_or(0);
                let n = attr_num(el, "replication")?.unwrap_or(1).max(1);
                let name = child_text(el, "name");
                let description = child_text(el, "description");
                let repnames: Vec<String> = el
                    .children()
                    .filter(|c| c.has_tag_name("repname"))
                    .filter_map(|c| c.text().map(|t| t.trim().to_string()))
                    .collect();
                if n == 1 {
                    items.push(Item::Group(Group {
                        name,
                        description,
                        items: parse_items(el, space, cursor)?,
                    }));
                } else {
                    let mut copies = Vec::new();
                    for i in 0..n {
                        let base = repnames
                            .get(i as usize)
                            .or(repnames.last())
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        copies.push(Item::Group(Group {
                            name: format!("{base} {}", i + 1).trim().to_string(),
                            description: String::new(),
                            items: parse_items(el, space, cursor)?,
                        }));
                    }
                    if name.is_empty() {
                        items.extend(copies);
                    } else {
                        items.push(Item::Group(Group {
                            name,
                            description,
                            items: copies,
                        }));
                    }
                }
                continue;
            }
            "int" => (
                size_of(1)?,
                FieldKind::Int {
                    min: child_num(el, "min"),
                    max: child_num(el, "max"),
                    default: child_num(el, "default"),
                    map: parse_map(el),
                },
            ),
            "string" => (size_of(0)?, FieldKind::String),
            "eventid" => (8, FieldKind::EventId),
            "float" => (
                size_of(4)?,
                FieldKind::Float {
                    min: child_num(el, "min"),
                    max: child_num(el, "max"),
                    default: child_num(el, "default"),
                },
            ),
            "action" => (
                size_of(1)?,
                FieldKind::Action {
                    button: {
                        let b = child_text(el, "buttonText");
                        if b.is_empty() {
                            "Run".into()
                        } else {
                            b
                        }
                    },
                    value: child_num(el, "value").unwrap_or(0),
                },
            ),
            "blob" => (size_of(0)?, FieldKind::Blob),
            _ => continue,
        };
        *cursor += attr_num(el, "offset")?.unwrap_or(0);
        if size < 0 || *cursor < 0 {
            return Err(format!("<{tag}> has a negative size or address"));
        }
        items.push(Item::Field(Field {
            name: child_text(el, "name"),
            description: child_text(el, "description"),
            space,
            address: *cursor as u32,
            size: size as u32,
            kind,
        }));
        *cursor += size;
    }
    Ok(items)
}

fn parse_map(el: roxmltree::Node) -> Vec<(i64, String)> {
    let Some(map) = el.children().find(|c| c.has_tag_name("map")) else {
        return Vec::new();
    };
    map.children()
        .filter(|c| c.has_tag_name("relation"))
        .filter_map(|r| {
            let v = child_num(r, "property")?;
            Some((v, child_text(r, "value")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_groups_and_replication() {
        let cdi = Cdi::parse(
            r#"<?xml version="1.0"?>
            <cdi>
              <identification><manufacturer>Acme</manufacturer><model>IO-8</model></identification>
              <acdi/>
              <segment space="253" origin="128">
                <name>Settings</name>
                <string size="16"><name>Note</name></string>
                <int size="1" offset="4"><name>Mode</name>
                  <map><relation><property>0</property><value>Off</value></relation>
                       <relation><property>1</property><value>On</value></relation></map>
                </int>
                <group replication="3" offset="2">
                  <name>Outputs</name><repname>Output</repname>
                  <int size="2"><name>Pulse</name><min>1</min><max>500</max></int>
                  <eventid><name>On</name></eventid>
                </group>
                <float><name>Gain</name></float>
              </segment>
              <segment space="0xFE"><blob size="4"/></segment>
            </cdi>"#,
        )
        .unwrap();
        assert_eq!(cdi.identification.model, "IO-8");
        // ACDI added a user-info segment first.
        assert_eq!(cdi.segments[0].space, SPACE_ACDI_USER);
        let s = &cdi.segments[1];
        assert_eq!(s.name, "Settings");
        let note = s.items[0].as_field().unwrap();
        assert_eq!((note.address, note.size), (128, 16));
        let mode = s.items[1].as_field().unwrap();
        assert_eq!(mode.address, 148);
        assert!(matches!(&mode.kind, FieldKind::Int { map, .. } if map.len() == 2));
        let outputs = s.items[2].as_group().unwrap();
        assert_eq!(outputs.name, "Outputs");
        assert_eq!(outputs.items.len(), 3);
        let third = outputs.items[2].as_group().unwrap();
        assert_eq!(third.name, "Output 3");
        // 149 + 2 offset = 151; each copy is 10 bytes.
        assert_eq!(third.items[0].as_field().unwrap().address, 151 + 20);
        assert_eq!(third.items[1].as_field().unwrap().address, 151 + 22);
        let gain = s.items[3].as_field().unwrap();
        assert_eq!((gain.address, gain.size), (181, 4));
        assert_eq!(cdi.segments[2].space, 0xFE);
        assert_eq!(cdi.fields().len(), 2 + 2 + 6 + 1 + 1);
    }

    #[test]
    fn trailing_nuls_are_ignored() {
        let cdi = Cdi::parse("<cdi><segment space=\"253\"/></cdi>\0\0\0junk").unwrap();
        assert_eq!(cdi.segments.len(), 1);
    }

    #[test]
    fn encode_and_decode() {
        let f = |kind, size| Field {
            name: String::new(),
            description: String::new(),
            space: 253,
            address: 0,
            size,
            kind,
        };
        let int = f(
            FieldKind::Int {
                min: Some(1),
                max: Some(1000),
                default: None,
                map: vec![],
            },
            2,
        );
        assert_eq!(int.encode("300").unwrap(), vec![0x01, 0x2C]);
        assert_eq!(int.decode(&[0x01, 0x2C]).unwrap(), "300");
        assert!(int.encode("0").is_err());
        assert!(int.encode("abc").is_err());

        let s = f(FieldKind::String, 8);
        assert_eq!(s.encode("Yard").unwrap(), b"Yard\0");
        assert!(s.encode("12345678").is_err());
        assert_eq!(s.decode(b"Yard\0\xFF\xFF\xFF").unwrap(), "Yard");

        let e = f(FieldKind::EventId, 8);
        let bytes = e.encode("05.01.01.01.22.00.00.FF").unwrap();
        assert_eq!(e.decode(&bytes).unwrap(), "05.01.01.01.22.00.00.FF");

        let fl = f(
            FieldKind::Float {
                min: None,
                max: None,
                default: None,
            },
            4,
        );
        assert_eq!(fl.decode(&fl.encode("1.5").unwrap()).unwrap(), "1.5");
        assert_eq!(int.decode(&[1]), None);
    }
}
