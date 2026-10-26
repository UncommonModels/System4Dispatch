//! LCC node configuration: the list of nodes on the bus, and a window per
//! node that shows its CDI as a form for reading and writing settings.

use dioxus::prelude::*;
use rolcb::NodeId;
use s4d_core::cdi::{Cdi, Field, FieldKind, Item};
use s4d_core::LinkState;

use crate::bus::Bus;
use crate::desktop::{Desktop, WinKind};
use crate::platform;

#[derive(Clone, PartialEq)]
struct NodeRow {
    id: NodeId,
    label: String,
    detail: String,
    has_cdi: bool,
}

/// Every node seen on the bus, with a button to configure it.
#[component]
pub fn NodesView() -> Element {
    let bus = use_context::<Bus>();
    let desk = use_context::<Desktop>();
    let rows = use_memo(move || {
        bus.view(|d| {
            let mut rows: Vec<NodeRow> = d
                .config
                .nodes
                .iter()
                .filter(|(_, n)| n.alias != 0)
                .map(|(id, n)| NodeRow {
                    id: *id,
                    label: n.label(*id),
                    detail: n
                        .info
                        .as_ref()
                        .map(|i| {
                            [
                                i.manufacturer.as_str(),
                                i.model.as_str(),
                                i.software_version.as_str(),
                            ]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .copied()
                            .collect::<Vec<_>>()
                            .join(" · ")
                        })
                        .unwrap_or_else(|| "asking…".into()),
                    has_cdi: n.has_cdi(),
                })
                .collect();
            rows.sort_by_key(|a| a.label.to_lowercase());
            rows
        })
    });
    let online = use_memo(move || bus.view(|d| d.link_state() == LinkState::Online));

    rsx! {
        div { class: "nodes-view",
            div { class: "buttons",
                button {
                    disabled: !online(),
                    onclick: move |_| bus.act(|d, _| d.discover_nodes()),
                    "⟳ Refresh"
                }
                span { class: "muted small",
                    if online() { "{rows.read().len()} nodes on the bus" } else { "Connect to see the nodes on the bus." }
                }
            }
            div { class: "node-list",
                for r in rows.read().iter().cloned() {
                    div { key: "{r.id}", class: "row node-row",
                        div { class: "name",
                            div { "{r.label}" }
                            div { class: "muted small", span { class: "mono", "{r.id}" } " · {r.detail}" }
                        }
                        button {
                            disabled: !r.has_cdi,
                            title: if r.has_cdi { "Read and change this node's settings" } else { "This node has no configuration description (CDI)" },
                            onclick: move |_| { desk.open(WinKind::Config(r.id)); },
                            "Configure"
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, PartialEq)]
enum CdiState {
    NotLoaded,
    Loading(usize),
    Loaded,
    Failed(String),
}

#[derive(Clone, PartialEq)]
struct Summary {
    label: String,
    detail: String,
    present: bool,
    online: bool,
    busy: usize,
    error: Option<String>,
    cdi: CdiState,
}

/// What the field forms need: which node, and its memory as read so far.
#[derive(Clone, Copy)]
struct CfgCtx {
    node: NodeId,
    memory: Memo<std::collections::BTreeMap<(u8, u32), u8>>,
    online: Memo<bool>,
}

/// One node's configuration, as described by its CDI.
#[component]
pub fn ConfigView(node: NodeId) -> Element {
    let bus = use_context::<Bus>();
    let summary = use_memo(move || {
        bus.view(|d| {
            let n = d.config.nodes.get(&node);
            let data = d.config.data.get(&node);
            let busy = d.config.busy(node);
            Summary {
                label: n.map(|n| n.label(node)).unwrap_or_else(|| node.to_string()),
                detail: n
                    .and_then(|n| n.info.as_ref())
                    .map(|i| {
                        format!(
                            "{} {} · hardware {} · software {}",
                            i.manufacturer, i.model, i.hardware_version, i.software_version
                        )
                    })
                    .unwrap_or_default(),
                present: n.is_some_and(|n| n.alias != 0),
                online: d.link_state() == LinkState::Online,
                busy,
                error: data.and_then(|x| x.error.clone()),
                cdi: match data.and_then(|x| x.cdi.as_ref()) {
                    Some(Ok(_)) => CdiState::Loaded,
                    Some(Err(e)) => CdiState::Failed(e.clone()),
                    None if busy > 0 => CdiState::Loading(data.map_or(0, |x| x.cdi_raw.len())),
                    None => CdiState::NotLoaded,
                },
            }
        })
    });
    // Cloned only when the CDI state changes, not on every bus message.
    let cdi = use_memo(move || {
        let loaded = summary.read().cdi == CdiState::Loaded;
        loaded
            .then(|| bus.peek(|d| d.config.data.get(&node).and_then(|x| x.cdi.clone()?.ok())))
            .flatten()
    });
    let memory = use_memo(move || {
        bus.view(|d| {
            d.config
                .data
                .get(&node)
                .map(|x| x.memory.clone())
                .unwrap_or_default()
        })
    });
    let online = use_memo(move || summary.read().online && summary.read().present);
    use_context_provider(|| CfgCtx {
        node,
        memory,
        online,
    });

    // Read the CDI when the window opens (and when the node comes back),
    // then the values that aren't inside a group.
    use_effect(move || {
        let s = summary.read();
        if s.online && s.present && s.cdi == CdiState::NotLoaded && s.busy == 0 && s.error.is_none()
        {
            bus.act(|d, _| d.load_cdi(node));
        }
    });
    use_effect(move || {
        if let Some(c) = cdi.read().as_ref() {
            let top: Vec<(u8, u32, u32)> = c
                .segments
                .iter()
                .flat_map(|s| s.items.iter().filter_map(Item::as_field))
                .map(|f| (f.space, f.address, f.size))
                .collect();
            bus.act(|d, _| d.read_config_fields(node, top));
        }
    });

    let s = summary.read().clone();
    rsx! {
        div { class: "config-view",
            div { class: "config-head",
                div { class: "config-title",
                    div { class: "config-name", "{s.label}" }
                    div { class: "muted small", span { class: "mono", "{node}" } if !s.detail.is_empty() { " · {s.detail}" } }
                }
                div { class: "buttons",
                    button {
                        disabled: !online() || cdi.read().is_none(),
                        title: "Read every value from the node",
                        onclick: move |_| bus.act(|d, _| d.read_all_config(node)),
                        "Read all"
                    }
                    button {
                        disabled: !online() || s.busy > 0,
                        title: "Download the node's configuration description again",
                        onclick: move |_| bus.act(|d, _| d.load_cdi(node)),
                        "Reload CDI"
                    }
                    button {
                        disabled: !online(),
                        title: "Restart the node, for settings that only take effect after a restart",
                        onclick: move |_| async move {
                            if platform::confirm("Restart this node?").await {
                                bus.act(|d, _| d.restart_node(node));
                            }
                        },
                        "Restart node"
                    }
                }
            }
            if !s.present {
                p { class: "warn", "This node isn't on the bus right now." }
            }
            if s.busy > 0 {
                p { class: "muted small working", "Working… {s.busy} request(s) left" }
            }
            if let Some(e) = &s.error {
                p { class: "warn", "{e}" }
            }
            match (&s.cdi, cdi.read().as_ref()) {
                (_, Some(c)) => rsx! { CdiTree { cdi: c.clone() } },
                (CdiState::Loading(n), _) => rsx! { p { class: "muted", "Reading the configuration description… {n} bytes" } },
                (CdiState::Failed(e), _) => rsx! { p { class: "warn", "The node's configuration description couldn't be read: {e}" } },
                _ => rsx! {},
            }
        }
    }
}

#[component]
fn CdiTree(cdi: Cdi) -> Element {
    rsx! {
        for (i, seg) in cdi.segments.iter().enumerate() {
            section { key: "{i}", class: "card cdi-segment",
                h3 {
                    if seg.name.is_empty() { "Space {seg.space:02X}" } else { "{seg.name}" }
                }
                if !seg.description.is_empty() { p { class: "muted small", "{seg.description}" } }
                CdiItems { items: seg.items.clone() }
            }
        }
    }
}

#[component]
fn CdiItems(items: Vec<Item>) -> Element {
    rsx! {
        for (i, item) in items.into_iter().enumerate() {
            match item {
                Item::Group(g) => rsx! { CdiGroup { key: "{i}", name: g.name, description: g.description, items: g.items } },
                Item::Field(f) => rsx! { CdiField { key: "{i}", field: f } },
            }
        }
    }
}

/// Every field inside `items`, as (space, address, size).
fn ranges(items: &[Item], out: &mut Vec<(u8, u32, u32)>) {
    for i in items {
        match i {
            Item::Field(f) => out.push((f.space, f.address, f.size)),
            Item::Group(g) => ranges(&g.items, out),
        }
    }
}

/// A collapsible group. Its contents are only built (and read from the
/// node) once it is opened, so big replicated CDIs stay quick.
#[component]
fn CdiGroup(name: String, description: String, items: Vec<Item>) -> Element {
    let bus = use_context::<Bus>();
    let ctx = use_context::<CfgCtx>();
    let mut open = use_signal(|| false);
    let items2 = items.clone();
    let toggle = move |_| {
        let now_open = !open();
        open.set(now_open);
        if now_open && (ctx.online)() {
            let mut r = Vec::new();
            ranges(&items2, &mut r);
            let mem = ctx.memory.peek();
            r.retain(|(s, a, n)| (0..*n).any(|i| !mem.contains_key(&(*s, a + i))));
            drop(mem);
            if !r.is_empty() {
                bus.act(|d, _| d.read_config_fields(ctx.node, r));
            }
        }
    };
    let title = if name.is_empty() {
        "Group".to_string()
    } else {
        name
    };
    rsx! {
        div { class: if open() { "cdi-group open" } else { "cdi-group" },
            button { class: "cdi-group-head", onclick: toggle,
                span { class: "chevron", if open() { "▾" } else { "▸" } }
                span { "{title}" }
            }
            if open() {
                div { class: "cdi-group-body",
                    if !description.is_empty() { p { class: "muted small", "{description}" } }
                    CdiItems { items }
                }
            }
        }
    }
}

/// One setting: its current value from the node, an editor, and a Write
/// button once it has been changed.
#[component]
fn CdiField(field: Field) -> Element {
    let bus = use_context::<Bus>();
    let ctx = use_context::<CfgCtx>();
    let mut edit = use_signal(|| None::<String>);
    let mut err = use_signal(|| None::<String>);
    let current = {
        let mem = ctx.memory.read();
        let bytes: Option<Vec<u8>> = (0..field.size)
            .map(|i| mem.get(&(field.space, field.address + i)).copied())
            .collect();
        bytes.and_then(|b| field.decode(&b))
    };
    let text = edit().or_else(|| current.clone()).unwrap_or_default();
    let dirty = edit().is_some_and(|e| Some(&e) != current.as_ref());
    let online = (ctx.online)();
    let node = ctx.node;

    let f = field.clone();
    let write = move |value: String| match f.encode(&value) {
        Ok(bytes) => {
            err.set(None);
            edit.set(Some(value));
            bus.act(|d, _| {
                d.write_config(node, f.space, f.address, &bytes);
                d.config_update_complete(node);
            });
        }
        Err(e) => err.set(Some(e)),
    };
    let mut write_btn = write.clone();
    let mut write_key = write.clone();
    let mut write_sel = write.clone();
    let f = field.clone();
    let read_one = move |_| {
        edit.set(None);
        err.set(None);
        bus.act(|d, _| d.read_config(node, f.space, f.address, f.size));
    };

    let control = match &field.kind {
        FieldKind::Int { map, .. } if !map.is_empty() => {
            let in_map = map.iter().any(|(v, _)| v.to_string() == text);
            rsx! {
                select {
                    disabled: !online,
                    onchange: move |e| write_sel(e.value()),
                    if !in_map {
                        option { value: "{text}", selected: true, if text.is_empty() { "—" } else { "{text}" } }
                    }
                    for (v, label) in map.iter() {
                        option { value: "{v}", selected: v.to_string() == text, "{label}" }
                    }
                }
            }
        }
        FieldKind::Action { button, .. } => {
            let button = button.clone();
            let name = field.name.clone();
            let w = write.clone();
            rsx! {
                button {
                    disabled: !online,
                    onclick: move |_| {
                        let mut w = w.clone();
                        let name = name.clone();
                        async move {
                            if platform::confirm(&format!("{}?", if name.is_empty() { "Run this" } else { &name })).await {
                                w(String::new());
                            }
                        }
                    },
                    "{button}"
                }
            }
        }
        FieldKind::Blob => {
            rsx! { code { class: "cdi-blob", if text.is_empty() { "—" } else { "{text}" } } }
        }
        kind => {
            let (ty, mono) = match kind {
                FieldKind::Int { .. } | FieldKind::Float { .. } => ("number", false),
                FieldKind::EventId => ("text", true),
                _ => ("text", false),
            };
            let (min, max) = match kind {
                FieldKind::Int { min, max, .. } => {
                    (min.map(|m| m.to_string()), max.map(|m| m.to_string()))
                }
                FieldKind::Float { min, max, .. } => {
                    (min.map(|m| m.to_string()), max.map(|m| m.to_string()))
                }
                _ => (None, None),
            };
            let maxlen =
                (*kind == FieldKind::String).then(|| field.size.saturating_sub(1).to_string());
            rsx! {
                input {
                    class: if mono { "mono" } else { "" },
                    r#type: "{ty}",
                    step: if ty == "number" && matches!(kind, FieldKind::Float { .. }) { "any" } else { "1" },
                    min: min,
                    max: max,
                    maxlength: maxlen,
                    placeholder: if current.is_none() { "not read" } else { "" },
                    value: "{text}",
                    oninput: move |e| edit.set(Some(e.value())),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            write_key(edit().unwrap_or_default());
                        } else if e.key() == Key::Escape {
                            edit.set(None);
                        }
                    },
                }
            }
        }
    };

    let label = if field.name.is_empty() {
        format!("@{:02X}:{}", field.space, field.address)
    } else {
        field.name.clone()
    };
    rsx! {
        div { class: "cdi-field",
            div { class: "cdi-label",
                span { title: "Space {field.space:02X}, address {field.address}, {field.size} bytes", "{label}" }
                if !field.description.is_empty() { div { class: "muted small", "{field.description}" } }
            }
            div { class: "cdi-control",
                {control}
                if dirty && !matches!(field.kind, FieldKind::Action { .. }) {
                    button { class: "primary", disabled: !online, title: "Write to the node (Enter)", onclick: move |_| write_btn(edit().unwrap_or_default()), "Write" }
                    button { class: "ghost", title: "Undo (Esc)", onclick: move |_| { edit.set(None); err.set(None); }, "↺" }
                } else if !matches!(field.kind, FieldKind::Action { .. }) {
                    button { class: "ghost", disabled: !online, title: "Read again from the node", onclick: read_one, "⟳" }
                }
            }
            if let Some(e) = err() { div { class: "warn small", "{e}" } }
        }
    }
}
