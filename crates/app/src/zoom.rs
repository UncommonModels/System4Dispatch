//! Zooming and panning a scrollable track drawing.
//!
//! The drawing sits in a scrolling container. Zoom is the size of one grid
//! cell in CSS pixels, or `None` to fit the drawing to the container's
//! width. A small script on the page does the parts that need the live DOM:
//! Ctrl+wheel (and trackpad pinch) zooming about the pointer, keeping the
//! same spot under the pointer as the drawing grows or shrinks, and
//! drag-to-pan. Zooming resizes the drawing in the browser straight away
//! and tells Rust the new size once the wheel stops, so a zoom gesture
//! doesn't re-render the drawing on every step.

use dioxus::prelude::*;

/// Smallest and largest cell sizes, in CSS pixels.
pub const MIN_PX: f32 = 4.0;
pub const MAX_PX: f32 = 96.0;

/// Zoom state for one drawing; see [`use_zoom_pan`].
#[derive(Clone, Copy)]
pub struct ZoomPan {
    /// Cell size in CSS pixels; `None` fits the drawing to the width.
    pub px: Signal<Option<f32>>,
    script: Signal<Option<document::Eval>>,
}

impl ZoomPan {
    /// Zoom in (`factor` > 1) or out about the middle of the view.
    pub fn zoom_by(&self, factor: f32) {
        if let Some(e) = *self.script.peek() {
            let _ = e.send(serde_json::json!({ "cmd": "zoom", "factor": factor }));
        }
    }

    /// Set the cell size directly (`None` fits to the width).
    pub fn set(&self, px: Option<f32>) {
        let px = px.map(|p| p.clamp(MIN_PX, MAX_PX));
        if let Some(e) = *self.script.peek() {
            let _ = e.send(serde_json::json!({ "cmd": "set", "px": px }));
        }
        let mut s = self.px;
        s.set(px);
    }

    /// Text for a zoom indicator: "Fit" or a percentage of 32 px cells.
    pub fn label(&self) -> String {
        match (self.px)() {
            None => "Fit".into(),
            Some(p) => format!("{:.0}%", p / 32.0 * 100.0),
        }
    }
}

#[derive(serde::Deserialize)]
struct ZoomMsg {
    px: Option<f32>,
}

/// Zoom and pan for the scrolling element matching `selector`, which holds
/// one `svg.track-svg`. With `left_drag_pans` a plain left-button drag
/// pans (for views where dragging does nothing else); the middle button
/// always pans.
pub fn use_zoom_pan(selector: &'static str, left_drag_pans: bool, initial: Option<f32>) -> ZoomPan {
    let px = use_signal(|| initial);
    let mut script = use_signal(|| None::<document::Eval>);
    use_hook(move || {
        let mut eval = document::eval(PAN_ZOOM);
        let _ = eval.send(serde_json::json!({
            "selector": selector,
            "leftPan": left_drag_pans,
            "min": MIN_PX,
            "max": MAX_PX,
        }));
        script.set(Some(eval));
        spawn(async move {
            let mut px = px;
            while let Ok(m) = eval.recv::<ZoomMsg>().await {
                let new = m.px.map(|p| p.clamp(MIN_PX, MAX_PX));
                if *px.peek() != new {
                    px.set(new);
                }
            }
        });
    });
    use_drop(move || {
        if let Some(e) = *script.peek() {
            let _ = e.send(serde_json::json!({ "cmd": "stop" }));
        }
    });
    ZoomPan { px, script }
}

/// The page side of [`use_zoom_pan`].
const PAN_ZOOM: &str = r#"
    const cfg = await dioxus.recv();
    const box = () => document.querySelector(cfg.selector);
    const svg = () => { const b = box(); return b && b.querySelector("svg.track-svg"); };
    const cellPx = (s) => s.getBoundingClientRect().width / s.viewBox.baseVal.width;

    // Resize the drawing now, keeping the content point under (ox, oy) in
    // place; tell Rust the new cell size once zooming pauses.
    let timer = null;
    const apply = (px, ox, oy) => {
        const b = box(), s = svg();
        if (!b || !s) return;
        if (ox === undefined) { ox = b.clientWidth / 2; oy = b.clientHeight / 2; }
        const w0 = s.getBoundingClientRect().width;
        const x = b.scrollLeft + ox, y = b.scrollTop + oy;
        const vb = s.viewBox.baseVal;
        if (px === null) {
            s.style.width = ""; s.style.height = "";
        } else {
            s.style.width = Math.round(vb.width * px) + "px";
            s.style.height = Math.round(vb.height * px) + "px";
        }
        const k = s.getBoundingClientRect().width / w0;
        b.scrollLeft = x * k - ox;
        b.scrollTop = y * k - oy;
    };
    const zoom = (factor, ox, oy) => {
        const s = svg();
        if (!s) return;
        const target = Math.min(cfg.max, Math.max(cfg.min, cellPx(s) * factor));
        apply(target, ox, oy);
        clearTimeout(timer);
        timer = setTimeout(() => dioxus.send({ px: target }), 180);
    };

    const onWheel = (e) => {
        const b = e.target.closest && e.target.closest(cfg.selector);
        if (!b || !(e.ctrlKey || e.metaKey)) return;
        e.preventDefault();
        const r = b.getBoundingClientRect();
        zoom(Math.exp(-e.deltaY * 0.0025), e.clientX - r.left, e.clientY - r.top);
    };

    let drag = null, swallowClick = false;
    const onDown = (e) => {
        const b = e.target.closest && e.target.closest(cfg.selector);
        if (!b) return;
        const pan = e.button === 1 || (cfg.leftPan && e.button === 0 && e.pointerType === "mouse");
        if (!pan) return;
        if (e.button === 1) e.preventDefault();
        drag = { b, x: e.clientX, y: e.clientY, sl: b.scrollLeft, st: b.scrollTop, moved: false };
    };
    const onMove = (e) => {
        if (!drag) return;
        const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
        if (!drag.moved && Math.hypot(dx, dy) < 5) return;
        drag.moved = true;
        drag.b.classList.add("panning");
        drag.b.scrollLeft = drag.sl - dx;
        drag.b.scrollTop = drag.st - dy;
    };
    const onUp = () => {
        if (drag && drag.moved) { swallowClick = true; setTimeout(() => swallowClick = false, 0); }
        if (drag) drag.b.classList.remove("panning");
        drag = null;
    };
    // A drag that panned must not also click a turnout.
    const onClick = (e) => { if (swallowClick) { e.stopPropagation(); e.preventDefault(); swallowClick = false; } };

    document.addEventListener("wheel", onWheel, { passive: false });
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("pointermove", onMove, true);
    document.addEventListener("pointerup", onUp, true);
    document.addEventListener("click", onClick, true);
    while (true) {
        const m = await dioxus.recv();
        if (m.cmd === "stop") break;
        if (m.cmd === "zoom") zoom(m.factor);
        if (m.cmd === "set") { clearTimeout(timer); apply(m.px); }
    }
    document.removeEventListener("wheel", onWheel, { passive: false });
    document.removeEventListener("pointerdown", onDown, true);
    document.removeEventListener("pointermove", onMove, true);
    document.removeEventListener("pointerup", onUp, true);
    document.removeEventListener("click", onClick, true);
"#;
