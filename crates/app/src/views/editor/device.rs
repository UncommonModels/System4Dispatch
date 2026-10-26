//! Picking the device output a block or turnout is wired to (a FemtoLCC
//! board and one of its outputs), which fills in its events.

use dioxus::prelude::*;
use rolcb::{EventId, NodeId};
use s4d_core::femto::{self, Output, OutputFields};
use s4d_core::layout::{Device, Feedback, Layout};
use s4d_core::LinkState;

use super::ops::*;
use crate::bus::Bus;
use crate::Draft;

/// What the device output is being picked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Block(String),
    Turnout(String),
}

impl Target {
    fn outputs(&self) -> Vec<Output> {
        match self {
            Target::Block(_) => Output::for_blocks(),
            Target::Turnout(_) => Output::for_turnouts(),
        }
    }

    /// The board's factory events for `out`, in [`OutputFields::events`]
    /// order.
    fn defaults(&self, node: NodeId, out: Output) -> Vec<EventId> {
        match (self, out) {
            (Target::Block(_), Output::Main(n)) => {
                let (occ, clr) = femto::default_block_events(node, n);
                vec![occ, clr]
            }
            (Target::Block(_), Output::Servo { .. }) => Vec::new(),
            (Target::Turnout(_), out) => {
                let t = femto::default_turnout_events(node, out);
                vec![t.normal, t.reverse, t.normal_feedback, t.reverse_feedback]
            }
        }
    }

    fn fields(&self, cdi: &s4d_core::cdi::Cdi, out: Output) -> Option<OutputFields> {
        match (self, out) {
            (Target::Block(_), Output::Main(n)) => femto::block_fields(cdi, n),
            (Target::Block(_), Output::Servo { .. }) => None,
            (Target::Turnout(_), out) => femto::turnout_fields(cdi, out),
        }
    }

    /// What the output is used as, in words.
    fn want(&self) -> &'static str {
        match self {
            Target::Block(_) => "a track block",
            Target::Turnout(_) => "a turnout",
        }
    }

    /// Save the device (when `Some`) and the events into the layout.
    fn store(&self, l: &mut Layout, device: Option<Option<Device>>, events: &[EventId]) {
        match self {
            Target::Block(id) => {
                if let Some(b) = l.blocks.iter_mut().find(|b| b.id == *id) {
                    if let Some(d) = device {
                        b.device = d;
                    }
                    if let [occ, clr] = events {
                        b.occupied_event = Some(*occ);
                        b.clear_event = Some(*clr);
                    }
                }
            }
            Target::Turnout(id) => {
                if let Some(t) = l.turnouts.iter_mut().find(|t| t.id == *id) {
                    if let Some(d) = device {
                        t.device = d;
                    }
                    if let [n, r, nf, rf] = events {
                        t.normal_event = Some(*n);
                        t.reverse_event = Some(*r);
                        t.feedback = Some(Feedback {
                            normal_event: Some(*nf),
                            reverse_event: Some(*rf),
                        });
                    }
                }
            }
        }
    }
}

/// Where reading a board's settings has got to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    /// Downloading the board's CDI, to find where the output's settings are.
    LoadingCdi(NodeId, Output),
    /// Reading the output's use and events.
    Reading(NodeId, Output),
}

/// A block's or turnout's device: a FemtoLCC board on the bus (or typed in
/// by node ID) and one of its outputs. Choosing both fills in the factory
/// events at once, then, if the board can be reached, the events it
/// actually uses.
#[component]
pub fn DevicePicker(target: Target, device: Option<Device>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let mut phase = use_signal(|| Phase::Idle);
    let mut status = use_signal(|| None::<(bool, String)>);
    // The output's use on the board, when it doesn't suit.
    let mut wrong_use = use_signal(|| None::<(NodeId, Output, String)>);
    let mut typing = use_signal(|| false);
    // The target as a signal, so the effect below sees the current one.
    let mut target_sig = use_signal(|| target.clone());
    if *target_sig.peek() != target {
        target_sig.set(target.clone());
    }

    let online = bus.view(|d| d.link_state() == LinkState::Online);
    let boards: Vec<(NodeId, String)> = bus.view(|d| {
        d.config
            .nodes
            .iter()
            .filter(|(_, r)| {
                r.info
                    .as_ref()
                    .is_some_and(|i| femto::is_femto(&i.manufacturer, &i.model))
            })
            .map(|(n, r)| (*n, r.label(*n)))
            .collect()
    });
    let saved_node = device.as_ref().map(|d| d.node);
    let saved_output = device.as_ref().and_then(|d| Output::parse(&d.output));

    // Start reading `out` of `node`: its CDI first if we don't have it.
    let mut start_read = move |node: NodeId, out: Output| {
        wrong_use.set(None);
        let reachable =
            bus.peek(|d| d.link_state() == LinkState::Online && d.config.nodes.contains_key(&node));
        if !reachable {
            phase.set(Phase::Idle);
            status.set(Some((
                false,
                "Filled in the board's factory events. Connect with the board on the bus to check them against its settings.".into(),
            )));
            return;
        }
        let have_cdi = bus.peek(|d| {
            d.config
                .data
                .get(&node)
                .is_some_and(|x| matches!(x.cdi, Some(Ok(_))))
        });
        if have_cdi {
            read_fields(bus, &target_sig.peek(), node, out);
            phase.set(Phase::Reading(node, out));
        } else {
            bus.act(|d, _| d.load_cdi(node));
            phase.set(Phase::LoadingCdi(node, out));
        }
        status.set(Some((
            false,
            format!("Reading {}'s settings from the board…", out.label()),
        )));
    };

    // Choose a board and output: save them, fill in the factory events and
    // go and read the real ones.
    let mut choose = move |node: Option<NodeId>, out: Option<Output>| {
        let target = target_sig.peek().clone();
        let dev = node.zip(out).map(|(n, o)| Device {
            model: femto::MODEL.into(),
            node: n,
            output: o.code(),
        });
        let events = node
            .zip(out)
            .map(|(n, o)| target.defaults(n, o))
            .unwrap_or_default();
        edit_draft(draft, bus, |l| target.store(l, Some(dev), &events));
        wrong_use.set(None);
        match node.zip(out) {
            Some((n, o)) => start_read(n, o),
            None => {
                phase.set(Phase::Idle);
                status.set(None);
            }
        }
    };

    // Follow the reads as the bus answers.
    use_effect(move || {
        let p = phase();
        let (node, out) = match p {
            Phase::Idle => return,
            Phase::LoadingCdi(n, o) | Phase::Reading(n, o) => (n, o),
        };
        // Re-runs whenever the dispatcher changes.
        if bus.view(|d| d.config.busy(node)) > 0 {
            return;
        }
        let target = target_sig.peek().clone();
        let fields = bus.peek(|d| {
            let data = d.config.data.get(&node);
            match data.map(|x| &x.cdi) {
                Some(Some(Ok(cdi))) => Ok(target.fields(cdi, out)),
                Some(Some(Err(e))) => Err(format!(
                    "its configuration description couldn't be read ({e})"
                )),
                _ => Err(data
                    .and_then(|x| x.error.clone())
                    .unwrap_or_else(|| "it didn't answer".into())),
            }
        });
        let fields = match fields {
            Ok(Some(f)) => f,
            Ok(None) => {
                phase.set(Phase::Idle);
                status.set(Some((
                    true,
                    format!(
                        "That node has no {} like a FemtoLCC's; the factory events are filled in.",
                        out.label()
                    ),
                )));
                return;
            }
            Err(why) => {
                phase.set(Phase::Idle);
                status.set(Some((
                    true,
                    format!("Couldn't read the board: {why}. The factory events are filled in."),
                )));
                return;
            }
        };
        if p == Phase::LoadingCdi(node, out) {
            read_fields(bus, &target, node, out);
            phase.set(Phase::Reading(node, out));
            return;
        }
        phase.set(Phase::Idle);
        let read = |f: &s4d_core::cdi::Field| {
            bus.peek(|d| {
                d.config
                    .data
                    .get(&node)
                    .and_then(|x| x.bytes(f.space, f.address, f.size))
            })
        };
        let role = read(&fields.role);
        let events: Option<Vec<EventId>> = fields
            .events
            .iter()
            .map(|f| {
                read(f).map(|b| EventId::new(u64::from_be_bytes(b.try_into().unwrap_or_default())))
            })
            .collect();
        let (Some(role), Some(events)) = (role, events) else {
            status.set(Some((
                true,
                "The board didn't send its settings; the factory events are filled in.".into(),
            )));
            return;
        };
        let factory = target.defaults(node, out) == events;
        edit_draft(draft, bus, |l| target.store(l, None, &events));
        status.set(Some((
            false,
            if factory {
                "Checked with the board: it uses the factory events.".into()
            } else {
                "Read from the board: it has been set to other events, which are filled in.".into()
            },
        )));
        if !role.first().is_some_and(|r| fields.suitable.contains(r)) {
            // Name the use from the field's map, else its number.
            let value = role.first().copied().unwrap_or_default() as i64;
            let what = match &fields.role.kind {
                s4d_core::cdi::FieldKind::Int { map, .. } => map
                    .iter()
                    .find(|(v, _)| *v == value)
                    .map(|(_, t)| t.clone()),
                _ => None,
            }
            .unwrap_or_else(|| value.to_string());
            wrong_use.set(Some((node, out, what)));
        }
    });

    let node_value = saved_node.map(|n| n.to_string()).unwrap_or_default();
    let known = saved_node.is_none_or(|n| boards.iter().any(|(b, _)| *b == n));
    let outputs = target.outputs();
    let want = target.want();
    rsx! {
        div { class: "device-pick",
            label { class: "field", span { if matches!(target, Target::Block(_)) { "Detected by" } else { "Driven by" } }
                select {
                    onchange: move |e| {
                        let v = e.value();
                        if v == "other" {
                            typing.set(true);
                            return;
                        }
                        typing.set(false);
                        let node = v.parse::<NodeId>().ok();
                        choose(node, node.and(saved_output.or(Some(Output::Main(0)))));
                    },
                    option { value: "", selected: saved_node.is_none() && !typing(), "— not set —" }
                    for (n, label) in boards.iter() {
                        option { value: "{n}", selected: saved_node == Some(*n) && !typing(), "FemtoLCC: {label}" }
                    }
                    if !known {
                        option { value: "{node_value}", selected: !typing(), "FemtoLCC {node_value} (not on the bus now)" }
                    }
                    option { value: "other", selected: typing(), "Another FemtoLCC, by node ID…" }
                }
            }
            if typing() {
                label { class: "field", span { "Node ID" }
                    input {
                        placeholder: "02.01.57.xx.xx.xx",
                        onchange: move |e| {
                            if let Ok(n) = e.value().trim().parse::<NodeId>() {
                                typing.set(false);
                                choose(Some(n), Some(saved_output.unwrap_or(Output::Main(0))));
                            } else {
                                status.set(Some((true, "That isn't a node ID: six pairs of hex digits, like 02.01.57.A7.CD.94.".into())));
                            }
                        }
                    }
                }
            }
            if let Some(n) = saved_node {
                label { class: "field", span { "Output" }
                    select {
                        onchange: move |e| {
                            if let Some(o) = Output::parse(&e.value()) {
                                choose(Some(n), Some(o));
                            }
                        },
                        for o in outputs.iter() {
                            option { value: "{o.code()}", selected: saved_output == Some(*o), "{o.label()}" }
                        }
                    }
                }
                if phase() == Phase::Idle {
                    button {
                        class: "text-btn",
                        disabled: !online,
                        title: if online { "Read the events from the board again" } else { "Connect to read from the board" },
                        onclick: move |_| start_read(n, saved_output.unwrap_or(Output::Main(0))),
                        "Read events from the board"
                    }
                }
            }
            if let Some((err, msg)) = status() {
                p { class: if err { "hint warn" } else { "hint" }, "{msg}" }
            }
            if let Some((node, out, what)) = wrong_use() {
                p { class: "hint warn",
                    "{out.label()} is set to “{what}” on the board, not {want}, so it won't work this way yet."
                }
                button {
                    class: "text-btn",
                    onclick: move |_| {
                        make_suitable(bus, &target_sig.peek(), node, out);
                        start_read(node, out);
                    },
                    "Make {out.label()} {want}"
                }
            }
        }
    }
}

/// The output's fields, from the board's CDI if it has been read.
fn fields_of(bus: Bus, target: &Target, node: NodeId, out: Output) -> Option<OutputFields> {
    bus.peek(|d| match d.config.data.get(&node).map(|x| &x.cdi) {
        Some(Some(Ok(cdi))) => target.fields(cdi, out),
        _ => None,
    })
}

fn read_fields(bus: Bus, target: &Target, node: NodeId, out: Output) {
    if let Some(f) = fields_of(bus, target, node, out) {
        let list = std::iter::once(&f.role)
            .chain(&f.events)
            .map(|x| (x.space, x.address, x.size))
            .collect();
        bus.act(|d, _| d.read_config_fields(node, list));
    }
}

/// Set the output's use on the board to suit the target.
fn make_suitable(bus: Bus, target: &Target, node: NodeId, out: Output) {
    if let Some(f) = fields_of(bus, target, node, out) {
        bus.act(|d, _| d.write_config(node, f.role.space, f.role.address, &[f.make_suitable]));
    }
}
