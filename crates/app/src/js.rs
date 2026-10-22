//! Browser glue implemented in small JavaScript snippets run through
//! `document::eval`. Each snippet talks to Rust over the eval channel.

use dioxus::document::{self, Eval};
use serde::Deserialize;

/// Messages from a link script (serial port or WebSocket).
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum LinkMsg {
    Open,
    Data { d: String },
    Closed { e: String },
    Error { e: String },
}

/// Sent to a link script to make it close the port.
pub const CLOSE: &str = "\u{0}close";

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Open a Web Serial port. `reuse` picks an already-authorised port without
/// prompting, if there is one.
pub fn open_serial(baud: u32, reuse: bool) -> Eval {
    let script = r#"
        if (!("serial" in navigator)) {
            dioxus.send({t: "error", e: "Web Serial is not available in this browser. Use Chrome or Edge, over https or localhost."});
            return;
        }
        let port;
        try {
            const known = await navigator.serial.getPorts();
            if (__REUSE__ && !known.length) {
                dioxus.send({t: "error", e: "Click Connect to choose your USB LCC adapter."});
                return;
            }
            port = __REUSE__ ? known[0] : await navigator.serial.requestPort();
            await port.open({ baudRate: __BAUD__ });
        } catch (e) {
            dioxus.send({t: "error", e: String(e.message || e)});
            return;
        }
        const reader = port.readable.getReader();
        const writer = port.writable.getWriter();
        const enc = new TextEncoder();
        const dec = new TextDecoder();
        let closing = false;
        (async () => {
            while (true) {
                const m = await dioxus.recv();
                if (m === __CLOSE__) { closing = true; try { await reader.cancel(); } catch (e) {} break; }
                try { await writer.write(enc.encode(m)); } catch (e) { dioxus.send({t: "error", e: String(e)}); }
            }
        })();
        dioxus.send({t: "open"});
        let why = "";
        try {
            while (true) {
                const { value, done } = await reader.read();
                if (done) break;
                dioxus.send({t: "data", d: dec.decode(value, { stream: true })});
            }
        } catch (e) { why = String(e.message || e); }
        try { reader.releaseLock(); writer.releaseLock(); await port.close(); } catch (e) {}
        dioxus.send({t: "closed", e: closing ? "" : (why || "serial port closed")});
    "#
    .replace("__REUSE__", if reuse { "true" } else { "false" })
    .replace("__BAUD__", &baud.to_string())
    .replace("__CLOSE__", &js_string(CLOSE));
    document::eval(&script)
}

/// Open a WebSocket carrying GridConnect text.
pub fn open_websocket(url: &str) -> Eval {
    let script = r#"
        let ws;
        try { ws = new WebSocket(__URL__); } catch (e) { dioxus.send({t: "error", e: String(e.message || e)}); return; }
        let closing = false;
        ws.onopen = () => dioxus.send({t: "open"});
        ws.onmessage = (ev) => { if (typeof ev.data === "string") dioxus.send({t: "data", d: ev.data}); };
        ws.onclose = (ev) => dioxus.send({t: "closed", e: closing ? "" : (ev.reason || ("connection closed (" + ev.code + ")"))});
        while (true) {
            const m = await dioxus.recv();
            if (m === __CLOSE__) { closing = true; ws.close(); break; }
            if (ws.readyState === 1) ws.send(m);
        }
    "#
    .replace("__URL__", &js_string(url))
    .replace("__CLOSE__", &js_string(CLOSE));
    document::eval(&script)
}

/// Default WebSocket URL: the `/ws` endpoint of whatever served this page.
pub async fn default_ws_url() -> String {
    document::eval(
        r#"const p = location.protocol === "https:" ? "wss://" : "ws://";
           return p + (location.host || "localhost:8080") + "/ws";"#,
    )
    .join::<String>()
    .await
    .unwrap_or_else(|_| "ws://localhost:8080/ws".into())
}

/// HTTP request; returns the body on a 2xx status.
pub async fn fetch(url: &str, method: &str, body: Option<&str>) -> Option<String> {
    let script = r#"
        try {
            const opts = { method: __METHOD__, cache: "no-store" };
            const body = __BODY__;
            if (body !== null) { opts.body = body; opts.headers = { "Content-Type": "application/json" }; }
            const r = await fetch(__URL__, opts);
            if (!r.ok) return null;
            return await r.text();
        } catch (e) { return null; }
    "#
    .replace("__METHOD__", &js_string(method))
    .replace("__BODY__", &body.map(js_string).unwrap_or_else(|| "null".into()))
    .replace("__URL__", &js_string(url));
    document::eval(&script)
        .join::<Option<String>>()
        .await
        .ok()
        .flatten()
}

/// Offer `text` as a file download.
pub fn download(filename: &str, text: &str) {
    let script = r#"
        const blob = new Blob([__TEXT__], { type: "application/json" });
        const a = document.createElement("a");
        a.href = URL.createObjectURL(blob);
        a.download = __NAME__;
        document.body.appendChild(a);
        a.click();
        setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
    "#
    .replace("__TEXT__", &js_string(text))
    .replace("__NAME__", &js_string(filename));
    let _ = document::eval(&script);
}

/// Ask the user to pick a file and return its text.
pub async fn pick_file(accept: &str) -> Option<String> {
    let script = r#"
        return await new Promise((resolve) => {
            const input = document.createElement("input");
            input.type = "file";
            input.accept = __ACCEPT__;
            input.onchange = async () => {
                const f = input.files && input.files[0];
                resolve(f ? await f.text() : null);
            };
            input.oncancel = () => resolve(null);
            input.click();
        });
    "#
    .replace("__ACCEPT__", &js_string(accept));
    document::eval(&script)
        .join::<Option<String>>()
        .await
        .ok()
        .flatten()
}

pub fn confirm(question: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(question).ok())
        .unwrap_or(true)
}

/// Milliseconds since the epoch (monotonic enough for protocol timers).
pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// Register the service worker so the app works offline / installs as a PWA.
pub fn register_service_worker() {
    let _ = document::eval(
        r#"if ("serviceWorker" in navigator && location.protocol === "https:") {
               navigator.serviceWorker.register("./sw.js").catch(() => {});
           }"#,
    );
}
