//! Owns the [`Dispatcher`] and the active link (simulator, USB serial or
//! GridConnect TCP), moves frames between them, and tells the UI when to
//! redraw.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use rolcb::gridconnect;
use rolcb::NodeId;
use s4d_core::sim::Simulator;
use s4d_core::{Dispatcher, Layout};

use crate::platform::{self, LinkMsg, LinkOut};
use crate::storage::{ConnKind, Settings};

#[derive(Debug, Clone, PartialEq)]
pub enum ConnStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Reported as the node's SNIP hardware version.
const HARDWARE_VERSION: &str = if cfg!(feature = "desktop") {
    "Desktop"
} else {
    "Web"
};

fn new_dispatcher(node_id: NodeId, layout: Layout) -> Dispatcher {
    let mut info = Dispatcher::default_info();
    info.hardware_version = HARDWARE_VERSION.into();
    Dispatcher::with_info(node_id, layout, info)
}

enum Link {
    None,
    Sim(Box<Simulator>),
    Remote(Box<dyn Fn(LinkOut)>),
}

struct Inner {
    disp: Dispatcher,
    link: Link,
    decoder: gridconnect::Decoder,
    last_rev: u64,
    /// Bumped on every connect/disconnect so stale link tasks can tell.
    generation: u64,
    /// The user wants to be connected (drives TCP auto-reconnect).
    wanted: bool,
}

/// Shared handle, provided as context at the app root.
#[derive(Clone, Copy)]
pub struct Bus {
    inner: CopyValue<Rc<RefCell<Inner>>>,
    /// Redraw trigger: bumped when the dispatcher's revision changes.
    pub rev: Signal<u64>,
    pub conn: Signal<ConnStatus>,
}

impl Bus {
    pub fn new(node_id: NodeId, layout: Layout) -> Bus {
        Bus {
            // Owned by the root scope: link tasks (spawn_forever) use them too.
            inner: CopyValue::new_in_scope(
                Rc::new(RefCell::new(Inner {
                    disp: new_dispatcher(node_id, layout),
                    link: Link::None,
                    decoder: gridconnect::Decoder::new(),
                    last_rev: 0,
                    generation: 0,
                    wanted: false,
                })),
                ScopeId::ROOT,
            ),
            rev: Signal::new_in_scope(0, ScopeId::ROOT),
            conn: Signal::new_in_scope(ConnStatus::Disconnected, ScopeId::ROOT),
        }
    }

    fn cell(&self) -> Rc<RefCell<Inner>> {
        self.inner.read().clone()
    }

    /// Read dispatcher state from a component; subscribes it to changes.
    pub fn view<R>(&self, f: impl FnOnce(&Dispatcher) -> R) -> R {
        let _ = self.rev.read();
        let cell = self.cell();
        let r = f(&cell.borrow().disp);
        r
    }

    /// Read without subscribing (event handlers).
    pub fn peek<R>(&self, f: impl FnOnce(&Dispatcher) -> R) -> R {
        let cell = self.cell();
        let r = f(&cell.borrow().disp);
        r
    }

    /// Mutate the dispatcher (throw, throttle...), then send what it queued.
    pub fn act<R>(&self, f: impl FnOnce(&mut Dispatcher, u64) -> R) -> R {
        let r = f(&mut self.cell().borrow_mut().disp, platform::now_ms());
        self.pump();
        r
    }

    /// Timer tick: protocol timers, simulator activity.
    pub fn tick(&self) {
        {
            let cell = self.cell();
            let mut i = cell.borrow_mut();
            let now = platform::now_ms();
            i.disp.poll(now);
            if let Link::Sim(sim) = &mut i.link {
                sim.poll(now);
            }
        }
        self.pump();
    }

    /// Move frames until both sides are quiet, then trigger a redraw if
    /// anything changed.
    fn pump(&self) {
        let cell = self.cell();
        {
            let mut guard = cell.borrow_mut();
            let i = &mut *guard;
            let now = platform::now_ms();
            for _ in 0..32 {
                let out = i.disp.take_outgoing();
                match &mut i.link {
                    Link::None => break,
                    Link::Remote(tx) => {
                        if !out.is_empty() {
                            let mut text = String::with_capacity(out.len() * 30);
                            for f in &out {
                                text.push_str(&gridconnect::format(f));
                                text.push('\n');
                            }
                            tx(LinkOut::Data(text));
                        }
                        break;
                    }
                    Link::Sim(sim) => {
                        for f in &out {
                            sim.receive(f, now);
                        }
                        let back = sim.take_outgoing();
                        if out.is_empty() && back.is_empty() {
                            break;
                        }
                        for f in &back {
                            i.disp.receive(f, now);
                        }
                    }
                }
            }
        }
        let changed = {
            let mut i = cell.borrow_mut();
            let rev = i.disp.revision;
            let c = i.last_rev != rev;
            i.last_rev = rev;
            c
        };
        if changed {
            let mut r = self.rev;
            *r.write() += 1;
        }
    }

    pub fn set_layout(&self, layout: Layout) {
        {
            let cell = self.cell();
            let mut i = cell.borrow_mut();
            if let Link::Sim(_) = i.link {
                // The simulator models the layout, so rebuild it. That is a
                // new bus: trains (and their aliases) from the old one are gone.
                let now = platform::now_ms();
                i.disp.link_down();
                i.link = Link::Sim(Box::new(Simulator::new(layout.clone(), now)));
                i.disp.set_layout(layout);
                i.disp.link_up(now);
            } else {
                i.disp.set_layout(layout);
            }
        }
        self.pump();
    }

    pub fn set_monitor(&self, on: bool) {
        self.cell().borrow_mut().disp.monitor = on;
    }

    /// Replace the dispatcher (e.g. after the Node ID changed). Open
    /// throttles release their trains first, while the old node can still
    /// talk to them.
    pub fn reset_node(&self, node_id: NodeId) {
        self.act(|d, _| {
            let ids: Vec<u32> = d.throttles.iter().map(|t| t.id).collect();
            for id in ids {
                d.release(id);
            }
        });
        let layout = self.peek(|d| d.layout().clone());
        self.disconnect();
        self.cell().borrow_mut().disp = new_dispatcher(node_id, layout);
        self.pump();
    }

    pub fn is_wanted(&self) -> bool {
        self.cell().borrow().wanted
    }

    fn set_conn(&self, s: ConnStatus) {
        let mut c = self.conn;
        c.set(s);
    }

    /// Open the configured connection. `auto` = not started by a click
    /// (Web Serial then only reuses an already-authorised port).
    pub fn connect(&self, settings: &Settings, auto: bool) {
        self.close_link();
        let gen = {
            let cell = self.cell();
            let mut i = cell.borrow_mut();
            i.generation += 1;
            i.wanted = true;
            i.generation
        };
        let link = match settings.conn {
            ConnKind::Simulator => {
                {
                    let cell = self.cell();
                    let mut i = cell.borrow_mut();
                    let now = platform::now_ms();
                    let layout = i.disp.layout().clone();
                    i.link = Link::Sim(Box::new(Simulator::new(layout, now)));
                    i.disp.link_up(now);
                }
                self.set_conn(ConnStatus::Connected);
                self.pump();
                return;
            }
            ConnKind::Serial => platform::open_serial(&settings.serial_port, settings.baud, auto),
            ConnKind::Tcp => platform::open_tcp(&settings.tcp_addr),
        };
        self.cell().borrow_mut().link = Link::Remote(link.tx);
        self.set_conn(ConnStatus::Connecting);
        let bus = *self;
        let settings = settings.clone();
        dioxus::core::spawn_forever(async move { bus.run_link(link.rx, gen, settings).await });
    }

    async fn run_link(&self, mut rx: UnboundedReceiver<LinkMsg>, gen: u64, settings: Settings) {
        let current = |b: &Bus| b.cell().borrow().generation == gen;
        loop {
            let msg = rx.next().await;
            if !current(self) {
                return;
            }
            match msg {
                Some(LinkMsg::Open) => {
                    self.cell().borrow_mut().disp.link_up(platform::now_ms());
                    self.set_conn(ConnStatus::Connected);
                    self.pump();
                }
                Some(LinkMsg::Data { d }) => {
                    {
                        let cell = self.cell();
                        let mut guard = cell.borrow_mut();
                        let i = &mut *guard;
                        let now = platform::now_ms();
                        for f in i.decoder.push(d.as_bytes()).into_iter().flatten() {
                            i.disp.receive(&f, now);
                        }
                    }
                    self.pump();
                }
                Some(LinkMsg::Closed { e }) | Some(LinkMsg::Error { e }) => {
                    self.link_lost(if e.is_empty() { None } else { Some(e) });
                    break;
                }
                None => {
                    self.link_lost(Some("connection ended".into()));
                    break;
                }
            }
        }
        // Network links come back by themselves; serial needs the user.
        if settings.conn == ConnKind::Tcp {
            platform::sleep_ms(3_000).await;
            if current(self) && self.is_wanted() {
                self.connect(&settings, true);
            }
        }
    }

    fn link_lost(&self, err: Option<String>) {
        {
            let cell = self.cell();
            let mut i = cell.borrow_mut();
            i.link = Link::None;
            i.disp.link_down();
            i.decoder = gridconnect::Decoder::new();
        }
        self.set_conn(match err {
            Some(e) => ConnStatus::Error(e),
            None => ConnStatus::Disconnected,
        });
        self.pump();
    }

    fn close_link(&self) {
        let cell = self.cell();
        let mut i = cell.borrow_mut();
        if let Link::Remote(tx) = &i.link {
            tx(LinkOut::Close);
        }
        i.link = Link::None;
    }

    /// User-initiated disconnect.
    pub fn disconnect(&self) {
        self.close_link();
        {
            let cell = self.cell();
            let mut i = cell.borrow_mut();
            i.generation += 1;
            i.wanted = false;
            i.disp.link_down();
        }
        self.set_conn(ConnStatus::Disconnected);
        self.pump();
    }
}
