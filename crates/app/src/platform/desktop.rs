//! Desktop build: native GridConnect over TCP and serial ports (tokio, which
//! Dioxus desktop already runs), settings as JSON files on disk, native file
//! dialogs.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::channel::mpsc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc as tmpsc;

use super::{Link, LinkMsg, LinkOut};

pub const TCP_AVAILABLE: bool = true;

/// Pump GridConnect text between a byte stream and the app until either
/// side closes.
async fn pump<S>(
    stream: S,
    ev: mpsc::UnboundedSender<LinkMsg>,
    mut out: tmpsc::UnboundedReceiver<LinkOut>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = ev.unbounded_send(LinkMsg::Open);
    let (mut rd, mut wr) = tokio::io::split(stream);
    let mut buf = [0u8; 4096];
    let why = loop {
        tokio::select! {
            n = rd.read(&mut buf) => match n {
                Ok(0) => break "connection closed".to_string(),
                Ok(n) => {
                    let _ = ev.unbounded_send(LinkMsg::Data { d: String::from_utf8_lossy(&buf[..n]).into_owned() });
                }
                Err(e) => break e.to_string(),
            },
            cmd = out.recv() => match cmd {
                Some(LinkOut::Data(t)) => {
                    if let Err(e) = wr.write_all(t.as_bytes()).await {
                        break e.to_string();
                    }
                }
                Some(LinkOut::Close) | None => break String::new(),
            },
        }
    };
    let _ = ev.unbounded_send(LinkMsg::Closed { e: why });
}

fn link(
    open: impl FnOnce(mpsc::UnboundedSender<LinkMsg>, tmpsc::UnboundedReceiver<LinkOut>)
        + Send
        + 'static,
) -> Link {
    let (ev_tx, rx) = mpsc::unbounded();
    let (out_tx, out_rx) = tmpsc::unbounded_channel();
    open(ev_tx, out_rx);
    Link {
        tx: Box::new(move |o| {
            let _ = out_tx.send(o);
        }),
        rx,
    }
}

/// Connect to a GridConnect TCP hub (JMRI, an LCC buffer, `rolcb-hub`).
pub fn open_tcp(addr: &str) -> Link {
    let addr = addr.trim().to_string();
    link(move |ev, out| {
        tokio::spawn(async move {
            match tokio::net::TcpStream::connect(&addr).await {
                Ok(sock) => {
                    let _ = sock.set_nodelay(true);
                    pump(sock, ev, out).await;
                }
                Err(e) => {
                    let _ = ev.unbounded_send(LinkMsg::Error {
                        e: format!("{addr}: {e}"),
                    });
                }
            }
        });
    })
}

/// Open a USB/serial GridConnect adapter by name (e.g. /dev/ttyACM0, COM3).
pub fn open_serial(port: &str, baud: u32, _reuse: bool) -> Link {
    use tokio_serial::SerialPortBuilderExt;
    let port = port.trim().to_string();
    link(move |ev, out| {
        tokio::spawn(async move {
            if port.is_empty() {
                let _ = ev.unbounded_send(LinkMsg::Error {
                    e: "Choose a serial port in Settings".into(),
                });
                return;
            }
            match tokio_serial::new(&port, baud).open_native_async() {
                Ok(s) => pump(s, ev, out).await,
                Err(e) => {
                    let _ = ev.unbounded_send(LinkMsg::Error {
                        e: format!("{port}: {e}"),
                    });
                }
            }
        });
    })
}

/// Serial ports present on this computer.
pub fn serial_ports() -> Vec<String> {
    tokio_serial::available_ports()
        .map(|v| v.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

pub fn serial_supported() -> bool {
    true
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn sleep_ms(ms: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

pub fn random_u16() -> u16 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let x = n ^ (std::process::id().rotate_left(16));
    (x ^ (x >> 16)) as u16
}

fn config_dir() -> Option<PathBuf> {
    let d = dirs::config_dir()?.join("System4Dispatch");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn key_path(key: &str) -> Option<PathBuf> {
    let name: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    Some(config_dir()?.join(format!("{name}.json")))
}

pub fn storage_get(key: &str) -> Option<String> {
    std::fs::read_to_string(key_path(key)?).ok()
}

pub fn storage_set(key: &str, value: &str) {
    if let Some(p) = key_path(key) {
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, value).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

/// Save `text` with a native "Save as" dialog.
pub fn save_file(filename: &str, text: &str) {
    let filename = filename.to_string();
    let text = text.to_string();
    tokio::spawn(async move {
        if let Some(f) = rfd::AsyncFileDialog::new()
            .set_file_name(&filename)
            .add_filter("Layout", &["json"])
            .save_file()
            .await
        {
            let _ = std::fs::write(f.path(), text);
        }
    });
}

/// Pick a JSON file with a native "Open" dialog and return its text.
pub async fn open_file() -> Option<String> {
    let f = rfd::AsyncFileDialog::new()
        .add_filter("Layout", &["json"])
        .pick_file()
        .await?;
    String::from_utf8(f.read().await).ok()
}

pub fn register_service_worker() {}

static SYSTEM_DARK: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();

/// The operating system's light/dark preference. WebKitGTK doesn't pass
/// GNOME's setting through to CSS, so ask the desktop portal directly.
///
/// The portal query is a blocking D-Bus call, so it runs once (first called
/// from `main`, before the UI starts) and is cached.
pub fn system_dark() -> Option<bool> {
    *SYSTEM_DARK.get_or_init(|| match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Some(true),
        Ok(dark_light::Mode::Light) => Some(false),
        _ => None,
    })
}

/// Theme the native window frame (and, on Linux, WebKit's own scheme) to
/// match the app.
pub fn set_native_theme(dark: Option<bool>) {
    use dioxus::desktop::tao::window::Theme;
    let theme = dark.map(|d| if d { Theme::Dark } else { Theme::Light });
    dioxus::desktop::window().window.set_theme(theme);
}
