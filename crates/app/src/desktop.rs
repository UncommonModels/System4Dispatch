//! The windowed desktop: the panel, throttles, node configuration and the
//! other views each live in a window that can be moved, resized,
//! minimised, maximised and closed. A dock along the bottom opens them and
//! switches between them.
//!
//! Window geometry lives in Rust ([`Win`]), but dragging and resizing are
//! done by a small script on the page that moves the window element
//! directly and reports the final geometry when the pointer is released,
//! so the views inside (the panel drawing in particular) don't re-render
//! on every pointer move.
//!
//! Windows snap while they are dragged: their edges stick to the desktop's
//! edges and to other windows, dropping one on the left or right edge
//! fills that half of the desktop (a corner fills a quarter), and dropping
//! it on the top edge maximises it. Dragging a snapped or maximised window
//! away gives it back its earlier size. Holding Shift turns snapping off.
//!
//! On narrow screens the stylesheet shows only the top window, filling the
//! desktop, and the dock switches between windows.
//!
//! Window arrangements are not saved: every start opens the panel.

use dioxus::prelude::*;
use rolcb::NodeId;
use serde::Deserialize;

use crate::bus::Bus;
use crate::views;

/// What a window shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WinKind {
    Panel,
    /// A throttle; `None` until a train is acquired.
    Throttle(Option<u32>),
    /// The list of nodes on the bus.
    Nodes,
    /// One node's configuration (CDI).
    Config(NodeId),
    Editor,
    Settings,
    Help,
    Monitor,
}

impl WinKind {
    /// Only one window of this kind may be open; opening it again focuses it.
    fn same_window(self, other: WinKind) -> bool {
        match (self, other) {
            (WinKind::Throttle(_), _) | (_, WinKind::Throttle(_)) => false,
            (WinKind::Config(a), WinKind::Config(b)) => a == b,
            (a, b) => std::mem::discriminant(&a) == std::mem::discriminant(&b),
        }
    }

    /// Default size (width, height) in CSS pixels, given the desktop's size.
    fn default_size(self, (aw, ah): (f64, f64)) -> (f64, f64) {
        let (w, h) = match self {
            WinKind::Panel => ((aw - 32.0).max(480.0) * 0.72, ah - 32.0),
            WinKind::Throttle(_) => (340.0, 640.0),
            WinKind::Nodes => (480.0, 440.0),
            WinKind::Config(_) => (600.0, 640.0),
            WinKind::Editor => (aw - 48.0, ah - 48.0),
            WinKind::Settings => (600.0, 680.0),
            WinKind::Help => (760.0, 680.0),
            WinKind::Monitor => (620.0, 380.0),
        };
        (w.min(aw - 16.0).max(260.0), h.min(ah - 16.0).max(200.0))
    }

    fn icon(self) -> &'static str {
        match self {
            WinKind::Panel => "▦",
            WinKind::Throttle(_) => "◉",
            WinKind::Nodes => "⛭",
            WinKind::Config(_) => "⚙",
            WinKind::Editor => "✎",
            WinKind::Settings => "☰",
            WinKind::Help => "?",
            WinKind::Monitor => "≋",
        }
    }
}

/// One open window.
#[derive(Debug, Clone, PartialEq)]
pub struct Win {
    pub id: u32,
    pub kind: WinKind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Stacking order: higher is in front.
    pub z: u32,
    pub minimized: bool,
    pub maximized: bool,
    /// The size to go back to when a window snapped to half or a quarter
    /// of the desktop is dragged away.
    pub restore: Option<(f64, f64)>,
}

/// The window manager, shared as context.
#[derive(Clone, Copy)]
pub struct Desktop {
    pub wins: Signal<Vec<Win>>,
    /// The desktop area's size in CSS pixels, reported by the page.
    area: Signal<(f64, f64)>,
    next_id: Signal<u32>,
    bus: Bus,
}

impl Desktop {
    fn new(bus: Bus) -> Desktop {
        Desktop {
            wins: Signal::new(Vec::new()),
            area: Signal::new((1200.0, 760.0)),
            next_id: Signal::new(1),
            bus,
        }
    }

    fn top_z(&self) -> u32 {
        self.wins.peek().iter().map(|w| w.z).max().unwrap_or(0)
    }

    /// The window in front (ignoring minimised ones).
    pub fn top(&self) -> Option<u32> {
        self.wins
            .read()
            .iter()
            .filter(|w| !w.minimized)
            .max_by_key(|w| w.z)
            .map(|w| w.id)
    }

    /// Open a window of `kind`, or bring an existing one to the front.
    pub fn open(&self, kind: WinKind) -> u32 {
        let existing = self
            .wins
            .peek()
            .iter()
            .find(|w| w.kind.same_window(kind))
            .map(|w| w.id);
        if let Some(id) = existing {
            self.focus(id);
            return id;
        }
        let area = *self.area.peek();
        let (w, h) = kind.default_size(area);
        let n = self.wins.peek().len() as f64;
        // Cascade new windows; the panel goes top left, throttles to its right.
        let (x, y) = match kind {
            WinKind::Panel => (16.0, 16.0),
            WinKind::Throttle(_) => {
                let k = self
                    .wins
                    .peek()
                    .iter()
                    .filter(|w| matches!(w.kind, WinKind::Throttle(_)))
                    .count() as f64;
                ((area.0 - w - 16.0 - k * 28.0).max(16.0), 16.0 + k * 28.0)
            }
            _ => (48.0 + (n % 8.0) * 28.0, 32.0 + (n % 8.0) * 28.0),
        };
        let mut next = self.next_id;
        let id = *next.peek();
        next.set(id + 1);
        let z = self.top_z() + 1;
        let mut wins = self.wins;
        wins.write().push(Win {
            id,
            kind,
            // Fit on the desktop.
            x: x.min(area.0 - w - 8.0).max(0.0),
            y: y.min(area.1 - h - 8.0).max(0.0),
            w,
            h,
            z,
            minimized: false,
            maximized: false,
            restore: None,
        });
        id
    }

    /// Bring a window to the front (restoring it if minimised).
    pub fn focus(&self, id: u32) {
        let top = self.top_z();
        let needs = self
            .wins
            .peek()
            .iter()
            .any(|w| w.id == id && (w.z != top || w.minimized));
        if !needs {
            return;
        }
        let mut wins = self.wins;
        if let Some(w) = wins.write().iter_mut().find(|w| w.id == id) {
            w.z = top + 1;
            w.minimized = false;
        };
    }

    /// Close a window. Closing a throttle releases its train.
    pub fn close(&self, id: u32) {
        let kind = self.wins.peek().iter().find(|w| w.id == id).map(|w| w.kind);
        if let Some(WinKind::Throttle(Some(t))) = kind {
            if !self.shared_throttle(id, t) {
                self.bus.act(|d, _| d.release(t));
            }
        }
        let mut wins = self.wins;
        wins.write().retain(|w| w.id != id);
    }

    /// Another window shows throttle `t` too.
    fn shared_throttle(&self, id: u32, t: u32) -> bool {
        self.wins
            .peek()
            .iter()
            .any(|w| w.id != id && w.kind == WinKind::Throttle(Some(t)))
    }

    pub fn minimize(&self, id: u32) {
        let mut wins = self.wins;
        if let Some(w) = wins.write().iter_mut().find(|w| w.id == id) {
            w.minimized = true;
        };
    }

    pub fn toggle_maximize(&self, id: u32) {
        let mut wins = self.wins;
        if let Some(w) = wins.write().iter_mut().find(|w| w.id == id) {
            w.maximized = !w.maximized;
        };
    }

    /// Dock button: bring to the front, or minimise if already in front.
    pub fn toggle(&self, id: u32) {
        if self.top_visible() == Some(id) {
            self.minimize(id);
        } else {
            self.focus(id);
        }
    }

    fn top_visible(&self) -> Option<u32> {
        self.wins
            .peek()
            .iter()
            .filter(|w| !w.minimized)
            .max_by_key(|w| w.z)
            .map(|w| w.id)
    }

    /// A throttle window acquired (or lost) its train. If another window
    /// already shows that train, focus it and close this one.
    pub fn set_throttle(&self, id: u32, t: Option<u32>) {
        if let Some(t) = t {
            let other = self
                .wins
                .peek()
                .iter()
                .find(|w| w.id != id && w.kind == WinKind::Throttle(Some(t)))
                .map(|w| w.id);
            if let Some(other) = other {
                let mut wins = self.wins;
                wins.write().retain(|w| w.id != id);
                self.focus(other);
                return;
            }
        }
        let mut wins = self.wins;
        if let Some(w) = wins.write().iter_mut().find(|w| w.id == id) {
            w.kind = WinKind::Throttle(t);
        };
    }

    /// A window was moved, resized or snapped by the page script.
    fn set_geometry(&self, id: u32, g: Geom) {
        let mut wins = self.wins;
        if let Some(win) = wins.write().iter_mut().find(|win| win.id == id) {
            (win.x, win.y, win.w, win.h) = (g.x, g.y, g.w, g.h);
            win.restore = g.restore;
            win.maximized = g.max;
        };
    }
}

/// Messages from the page script.
#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum PageMsg {
    Geom {
        id: u32,
        #[serde(flatten)]
        geom: Geom,
    },
    Area {
        w: f64,
        h: f64,
    },
}

/// A window's geometry after a drag.
#[derive(Deserialize)]
struct Geom {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// The size before it snapped to part of the desktop.
    #[serde(default)]
    restore: Option<(f64, f64)>,
    /// Dropped on the top edge.
    #[serde(default)]
    max: bool,
}

/// Provide the [`Desktop`] context and open the starting windows.
pub fn use_desktop(bus: Bus) -> Desktop {
    let desk = use_context_provider(|| Desktop::new(bus));
    use_hook(move || {
        let mut eval = document::eval(WINDOW_SCRIPT);
        spawn(async move {
            let mut first = true;
            while let Ok(m) = eval.recv::<PageMsg>().await {
                match m {
                    PageMsg::Geom { id, geom } => desk.set_geometry(id, geom),
                    PageMsg::Area { w, h } => {
                        let mut area = desk.area;
                        area.set((w, h));
                        if first {
                            first = false;
                            desk.open(WinKind::Panel);
                        }
                    }
                }
            }
        });
    });
    desk
}

/// The desktop area with its windows.
#[component]
pub fn DesktopArea() -> Element {
    let desk = use_context::<Desktop>();
    let wins = desk.wins.read().clone();
    let top = desk.top();
    let empty = wins.iter().all(|w| w.minimized);
    rsx! {
        div { class: "desktop",
            if empty {
                div { class: "desktop-empty",
                    p { "No windows open." }
                    p { class: "muted", "Open the panel, a throttle or a tool from the dock below." }
                }
            }
            for w in wins {
                WinFrame { key: "{w.id}", is_top: top == Some(w.id), win: w }
            }
        }
    }
}

#[component]
fn WinFrame(win: Win, is_top: bool) -> Element {
    let desk = use_context::<Desktop>();
    let id = win.id;
    let mut class = String::from("win");
    if win.minimized {
        class.push_str(" min");
    }
    if win.maximized {
        class.push_str(" max");
    }
    if is_top {
        class.push_str(" top");
    }
    let style = format!(
        "left:{}px;top:{}px;width:{}px;height:{}px;z-index:{}",
        win.x, win.y, win.w, win.h, win.z
    );
    // The size a drag gives back: a maximised window's own, or the one
    // from before it snapped.
    let restore = match (win.maximized, win.restore) {
        (true, _) => format!("{},{}", win.w, win.h),
        (false, Some((w, h))) => format!("{w},{h}"),
        (false, None) => String::new(),
    };
    rsx! {
        div {
            class: "{class}",
            style: "{style}",
            "data-win": "{id}",
            "data-restore": "{restore}",
            onpointerdown: move |_| desk.focus(id),
            div { class: "win-title",
                ondoubleclick: move |_| desk.toggle_maximize(id),
                span { class: "win-icon", "{win.kind.icon()}" }
                WinTitle { kind: win.kind }
                div { class: "win-buttons",
                    button { title: "Minimise", onclick: move |e| { e.stop_propagation(); desk.minimize(id) }, "–" }
                    button {
                        class: "win-max",
                        title: if win.maximized { "Restore" } else { "Maximise" },
                        onclick: move |_| desk.toggle_maximize(id),
                        if win.maximized { "❐" } else { "□" }
                    }
                    button { class: "win-close", title: "Close", onclick: move |e| { e.stop_propagation(); desk.close(id) }, "✕" }
                }
            }
            div { class: "win-body",
                match win.kind {
                    WinKind::Panel => rsx! { views::panel::PanelView {} },
                    WinKind::Throttle(t) => rsx! { views::throttle::ThrottleWindow { win: id, throttle: t } },
                    WinKind::Nodes => rsx! { views::nodes::NodesView {} },
                    WinKind::Config(n) => rsx! { views::nodes::ConfigView { node: n } },
                    WinKind::Editor => rsx! { views::editor::EditorView {} },
                    WinKind::Settings => rsx! { views::settings::SettingsView {} },
                    WinKind::Help => rsx! { views::help::HelpView {} },
                    WinKind::Monitor => rsx! { views::panel::MonitorWindow {} },
                }
            }
            div { class: "win-resize", title: "Drag to resize" }
        }
    }
}

/// A window's title; a component of its own because throttle and node
/// titles follow the bus.
#[component]
fn WinTitle(kind: WinKind) -> Element {
    let bus = use_context::<Bus>();
    let text = match kind {
        WinKind::Panel => bus.view(|d| format!("Panel · {}", d.layout().name)),
        WinKind::Throttle(t) => throttle_title(bus, t),
        WinKind::Nodes => "LCC nodes".into(),
        WinKind::Config(n) => bus.view(|d| match d.config.nodes.get(&n) {
            Some(r) => format!("Configure · {}", r.label(n)),
            None => format!("Configure · {n}"),
        }),
        WinKind::Editor => "Layout editor".into(),
        WinKind::Settings => "Settings".into(),
        WinKind::Help => "Help".into(),
        WinKind::Monitor => "Bus monitor".into(),
    };
    rsx! { span { class: "win-name", title: "{text}", "{text}" } }
}

fn throttle_title(bus: Bus, t: Option<u32>) -> String {
    let addr =
        t.and_then(|t| bus.view(|d| d.throttles.iter().find(|x| x.id == t).map(|x| x.address)));
    match addr {
        Some(a) => format!("Throttle · {a}"),
        None => "Throttle".into(),
    }
}

/// The dock: launchers for each kind of window, then a button for every
/// throttle and node configuration window.
#[component]
pub fn Dock() -> Element {
    let desk = use_context::<Desktop>();
    let bus = use_context::<Bus>();
    let wins = desk.wins.read().clone();
    let top = desk.top();
    let launchers = [
        (WinKind::Panel, "Panel"),
        (WinKind::Nodes, "Nodes"),
        (WinKind::Editor, "Editor"),
        (WinKind::Settings, "Settings"),
        (WinKind::Monitor, "Monitor"),
        (WinKind::Help, "Help"),
    ];
    let launch = move |kind: WinKind| {
        let open = desk
            .wins
            .peek()
            .iter()
            .find(|w| w.kind.same_window(kind))
            .map(|w| w.id);
        match open {
            Some(id) => desk.toggle(id),
            None => {
                desk.open(kind);
            }
        }
    };
    rsx! {
        nav { class: "dock",
            for (kind, label) in launchers {
                {
                    let w = wins.iter().find(|w| w.kind.same_window(kind));
                    let class = match w {
                        Some(w) if top == Some(w.id) => "on open",
                        Some(_) => "open",
                        None => "",
                    };
                    rsx! {
                        button { class: "{class}", onclick: move |_| launch(kind),
                            span { class: "icon", "{kind.icon()}" }
                            span { "{label}" }
                        }
                    }
                }
            }
            div { class: "dock-sep" }
            button {
                class: "dock-new",
                title: "Open a new throttle",
                onclick: move |_| { desk.open(WinKind::Throttle(None)); },
                span { class: "icon", "◉" }
                span { "+ Throttle" }
            }
            for w in wins.iter().filter(|w| matches!(w.kind, WinKind::Throttle(_) | WinKind::Config(_))) {
                {
                    let id = w.id;
                    let label = match w.kind {
                        WinKind::Throttle(t) => throttle_title(bus, t).replace("Throttle · ", ""),
                        WinKind::Config(n) => bus.view(|d| {
                            d.config.nodes.get(&n).map(|r| r.label(n)).unwrap_or_else(|| n.to_string())
                        }),
                        _ => String::new(),
                    };
                    let class = if top == Some(id) { "task on open" } else { "task open" };
                    rsx! {
                        button { key: "{id}", class: "{class}", title: "{label}", onclick: move |_| desk.toggle(id),
                            span { class: "icon", "{w.kind.icon()}" }
                            span { class: "task-label", "{label}" }
                        }
                    }
                }
            }
        }
    }
}

/// The page side of the window manager: drag to move or resize, and
/// report the desktop's size.
const WINDOW_SCRIPT: &str = r#"
    const desk = () => document.querySelector(".desktop");
    // Snapping: how close (px) an edge must come to stick, how close the
    // pointer must come to the desktop's edge to snap to a half, and how
    // near a corner it must be to snap to a quarter.
    const STICK = 10, EDGE = 8, CORNER = 90;
    let op = null, preview = null;

    // Pull `start` (or `start + size`) onto the nearest target in reach.
    const stick = (start, size, targets) => {
        let best = null;
        for (const t of targets) {
            for (const d of [t - start, t - start - size]) {
                if (Math.abs(d) <= STICK && (best === null || Math.abs(d) < Math.abs(best))) best = d;
            }
        }
        return best === null ? start : start + best;
    };
    // Edges to stick to: the desktop's, and those of other windows that
    // are level with (or beside) the span being moved.
    const targets = (l, t, w, h) => {
        const xs = [0, op.dw], ys = [0, op.dh];
        for (const o of op.others) {
            if (t < o.b + STICK && t + h > o.t - STICK) xs.push(o.l, o.r);
            if (l < o.r + STICK && l + w > o.l - STICK) ys.push(o.t, o.b);
        }
        return { xs, ys };
    };
    // The part of the desktop the pointer would snap the window to, if any.
    const zoneAt = (e) => {
        const r = op.rect, dw = op.dw, dh = op.dh;
        const hw = Math.round(dw / 2), hh = Math.round(dh / 2);
        const left = e.clientX <= r.left + EDGE, right = e.clientX >= r.left + dw - EDGE;
        if (left || right) {
            const x = left ? 0 : hw, w = left ? hw : dw - hw;
            if (e.clientY < r.top + CORNER) return { x, y: 0, w, h: hh };
            if (e.clientY > r.top + dh - CORNER) return { x, y: hh, w, h: dh - hh };
            return { x, y: 0, w, h: dh };
        }
        if (e.clientY <= r.top + EDGE) return { x: 0, y: 0, w: dw, h: dh, max: true };
        return null;
    };
    const showZone = (z) => {
        if (!z) { if (preview) preview.style.display = "none"; return; }
        if (!preview) {
            preview = document.createElement("div");
            preview.className = "snap-preview";
            document.body.appendChild(preview);
        }
        Object.assign(preview.style, { display: "block",
            left: op.rect.left + z.x + "px", top: op.rect.top + z.y + "px",
            width: z.w + "px", height: z.h + "px" });
    };

    const onDown = (e) => {
        if (e.button !== 0) return;
        const t = e.target;
        const win = t.closest && t.closest(".win");
        if (!win) return;
        let mode = null;
        if (t.closest(".win-resize")) mode = "resize";
        else if (t.closest(".win-title") && !t.closest("button")) mode = "move";
        if (!mode) return;
        // Phone layout: windows fill the screen and don't move.
        if (getComputedStyle(win.querySelector(".win-max")).display === "none") return;
        const d = desk();
        e.preventDefault();
        const others = [...d.querySelectorAll(".win")]
            .filter((o) => o !== win && !o.classList.contains("min"))
            .map((o) => ({ l: o.offsetLeft, t: o.offsetTop, r: o.offsetLeft + o.offsetWidth, b: o.offsetTop + o.offsetHeight }));
        const restore = win.dataset.restore ? win.dataset.restore.split(",").map(Number) : null;
        op = { win, mode, x: e.clientX, y: e.clientY,
               l: win.offsetLeft, t: win.offsetTop, w: win.offsetWidth, h: win.offsetHeight,
               dw: d.clientWidth, dh: d.clientHeight, rect: d.getBoundingClientRect(),
               others, restore, zone: null, moved: false };
    };
    const onMove = (e) => {
        if (!op) return;
        const dx = e.clientX - op.x, dy = e.clientY - op.y;
        if (!op.moved && Math.hypot(dx, dy) < 3) return;
        const s = op.win.style;
        if (!op.moved) {
            op.moved = true;
            op.win.classList.add("dragging");
            // A snapped or maximised window gets its earlier size back,
            // keeping the same spot of its title bar under the pointer.
            if (op.mode === "move" && op.restore) {
                const [rw, rh] = op.restore;
                const along = (op.x - op.rect.left - op.l) / op.w;
                op.win.classList.remove("max");
                op.l = Math.round(op.x - op.rect.left - along * rw);
                op.w = rw; op.h = rh;
                s.width = rw + "px"; s.height = rh + "px";
            }
        }
        const free = e.shiftKey;
        if (op.mode === "move") {
            let l = op.l + dx, t = op.t + dy;
            if (!free) {
                const { xs, ys } = targets(l, t, op.w, op.h);
                l = stick(l, op.w, xs);
                t = stick(t, op.h, ys);
            }
            // Keep the title bar reachable.
            s.left = Math.min(Math.max(l, 80 - op.w), op.dw - 80) + "px";
            s.top = Math.min(Math.max(t, 0), op.dh - 36) + "px";
            op.zone = free ? null : zoneAt(e);
            showZone(op.zone);
        } else {
            let r = op.l + Math.max(240, op.w + dx), b = op.t + Math.max(160, op.h + dy);
            if (!free) {
                const { xs, ys } = targets(op.l, op.t, r - op.l, b - op.t);
                r = stick(r, 0, xs);
                b = stick(b, 0, ys);
            }
            s.width = Math.max(240, r - op.l) + "px";
            s.height = Math.max(160, b - op.t) + "px";
        }
    };
    const onUp = () => {
        if (!op) return;
        const w = op.win, s = w.style, z = op.zone;
        w.classList.remove("dragging");
        showZone(null);
        if (op.moved) {
            // A window that snapped got its earlier size back when the drag
            // began, so that's the size to remember.
            const g = { t: "geom", id: +w.dataset.win, x: w.offsetLeft, y: w.offsetTop,
                        w: w.offsetWidth, h: w.offsetHeight, restore: null, max: false };
            if (z && z.max) {
                // Maximised: it keeps its own size and place for later.
                g.max = true;
                w.classList.add("max");
            } else if (z) {
                g.restore = [g.w, g.h];
                Object.assign(g, { x: z.x, y: z.y, w: z.w, h: z.h });
                Object.assign(s, { left: z.x + "px", top: z.y + "px", width: z.w + "px", height: z.h + "px" });
            }
            dioxus.send(g);
        }
        op = null;
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("pointermove", onMove, true);
    document.addEventListener("pointerup", onUp, true);
    document.addEventListener("pointercancel", onUp, true);

    // Report the desktop's size once the stylesheet has laid it out (the
    // first windows are sized from it), then whenever it changes.
    const ready = (d) => d && getComputedStyle(d).position === "relative" && d.clientHeight > 0;
    const report = (d) => dioxus.send({ t: "area", w: d.clientWidth, h: d.clientHeight });
    const wait = () => {
        const d = desk();
        if (!ready(d)) return requestAnimationFrame(wait);
        report(d);
        new ResizeObserver(() => report(d)).observe(d);
    };
    wait();
    await new Promise(() => {});
"#;
