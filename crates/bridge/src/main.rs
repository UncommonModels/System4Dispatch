//! `s4d-bridge` command line.
//!
//! ```text
//! s4d-bridge --upstream jmri.local:12021          # relay to a JMRI hub
//! s4d-bridge --serial /dev/ttyACM0                # Pi with a USB LCC buffer
//! s4d-bridge --serial /dev/ttyACM0 --layout club.json
//! ```
//! Then open http://<this machine>:8080 on any phone, tablet or PC.

use std::path::PathBuf;

use clap::Parser;
use rolcb::hub::{self, Hub};
use s4d_bridge::{router, AppState};
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(version, about = "System4Dispatch bridge: web app + OpenLCB hub")]
struct Args {
    /// Address for the web app and its WebSocket.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,
    /// Also accept GridConnect TCP clients (JMRI, other tools) here.
    #[arg(long, default_value = "0.0.0.0:12021")]
    tcp: String,
    /// Don't accept GridConnect TCP clients.
    #[arg(long)]
    no_tcp: bool,
    /// Upstream GridConnect TCP hub(s), host:port. Repeatable.
    #[arg(long)]
    upstream: Vec<String>,
    /// USB/serial GridConnect adapter(s), e.g. /dev/ttyACM0 or COM3. Repeatable.
    #[arg(long)]
    serial: Vec<String>,
    /// Serial baud rate.
    #[arg(long, default_value_t = 115200)]
    baud: u32,
    /// Shared layout file that every connected panel loads and saves.
    #[arg(long)]
    layout: Option<PathBuf>,
    /// Serve the web app from this directory instead of the embedded copy.
    #[arg(long)]
    static_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let hub = Hub::new();
    let mut uplinks = Vec::new();

    if !args.no_tcp {
        let l = TcpListener::bind(&args.tcp).await?;
        eprintln!(
            "[s4d-bridge] GridConnect TCP clients on {}",
            l.local_addr()?
        );
        tokio::spawn(hub::serve_tcp(hub.clone(), l));
    }
    for addr in &args.upstream {
        uplinks.push(format!("tcp {addr}"));
        tokio::spawn(hub::uplink_tcp(hub.clone(), addr.clone()));
    }
    for path in &args.serial {
        uplinks.push(format!("serial {path}"));
        tokio::spawn(hub::uplink_serial(hub.clone(), path.clone(), args.baud));
    }
    if let Some(p) = &args.layout {
        if !p.exists() {
            std::fs::write(p, s4d_core::Layout::demo().to_json())?;
            eprintln!("[s4d-bridge] created {} from the demo layout", p.display());
        }
    }

    let state = AppState {
        hub,
        layout_path: args.layout,
        uplinks,
        static_dir: args.static_dir,
    };
    let listener = TcpListener::bind(&args.listen).await?;
    let addr = listener.local_addr()?;
    eprintln!("[s4d-bridge] open http://{addr} (WebSocket at /ws)");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
