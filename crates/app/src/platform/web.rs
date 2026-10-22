//! Web build: Web Serial and file handling through small JavaScript
//! snippets run with `document::eval`; settings in localStorage.

use dioxus::document;
use futures::channel::mpsc;

use super::{Link, LinkMsg, LinkOut};

/// Browsers can't open raw TCP sockets.
pub const TCP_AVAILABLE: bool = false;

const CLOSE: &str = "\u{0}close";

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Open a Web Serial port. `reuse` picks an already-authorised port without
/// prompting (needed for auto-connect, which has no user click); without it
/// the browser shows its port picker. `_port` is unused on the web.
pub fn open_serial(_port: &str, baud: u32, reuse: bool) -> Link {
    let script = r#"
        if (!("serial" in navigator)) {
            dioxus.send({t: "error", e: "USB serial needs Chrome or Edge on a desktop computer, with the page served over https or from localhost."});
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
    let eval = document::eval(&script);

    let (ev_tx, rx) = mpsc::unbounded();
    dioxus::core::spawn_forever(async move {
        let mut eval = eval;
        loop {
            match eval.recv::<LinkMsg>().await {
                Ok(m) => {
                    let end = matches!(m, LinkMsg::Closed { .. } | LinkMsg::Error { .. });
                    let _ = ev_tx.unbounded_send(m);
                    if end {
                        break;
                    }
                }
                Err(_) => {
                    let _ = ev_tx.unbounded_send(LinkMsg::Closed {
                        e: "connection ended".into(),
                    });
                    break;
                }
            }
        }
    });
    Link {
        tx: Box::new(move |o| {
            let _ = match o {
                LinkOut::Data(t) => eval.send(t),
                LinkOut::Close => eval.send(CLOSE),
            };
        }),
        rx,
    }
}

/// Not available in browsers; reports an error immediately.
pub fn open_tcp(_addr: &str) -> Link {
    let (ev_tx, rx) = mpsc::unbounded();
    let _ = ev_tx.unbounded_send(LinkMsg::Error {
        e: "GridConnect TCP needs the desktop app".into(),
    });
    Link {
        tx: Box::new(|_| {}),
        rx,
    }
}

/// Port names aren't visible to web pages; the browser shows a picker.
pub fn serial_ports() -> Vec<String> {
    Vec::new()
}

pub fn serial_supported() -> bool {
    let Some(w) = web_sys::window() else {
        return false;
    };
    js_sys::Reflect::has(&w.navigator(), &"serial".into()).unwrap_or(false)
}

pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

pub async fn sleep_ms(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}

pub fn random_u16() -> u16 {
    (js_sys::Math::random() * 65536.0) as u16
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn storage_get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn storage_set(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

/// Offer `text` as a file download.
pub fn save_file(filename: &str, text: &str) {
    let script = r#"
        const blob = new Blob([__TEXT__], { type: "application/json" });
        const a = document.createElement("a");
        a.href = URL.createObjectURL(blob);
        a.download = __NAME__;
        document.body.appendChild(a);
        a.click();
        setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
    "#
    .replace("__NAME__", &js_string(filename))
    .replace("__TEXT__", &js_string(text));
    let _ = document::eval(&script);
}

/// Ask the user to pick a JSON file and return its text.
pub async fn open_file() -> Option<String> {
    let script = r#"
        return await new Promise((resolve) => {
            const input = document.createElement("input");
            input.type = "file";
            input.accept = ".json,application/json";
            input.onchange = async () => {
                const f = input.files && input.files[0];
                resolve(f ? await f.text() : null);
            };
            input.oncancel = () => resolve(null);
            input.click();
        });
    "#;
    document::eval(script)
        .join::<Option<String>>()
        .await
        .ok()
        .flatten()
}

/// Register the service worker so the app works offline / installs as a PWA.
pub fn register_service_worker() {
    let _ = document::eval(
        r#"if ("serviceWorker" in navigator && location.protocol === "https:") {
               navigator.serviceWorker.register("./sw.js").catch(() => {});
           }"#,
    );
}

/// Browsers report the system scheme to CSS directly.
pub fn system_dark() -> Option<bool> {
    None
}

pub fn set_native_theme(_dark: Option<bool>) {}
