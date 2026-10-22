//! The bridge relays GridConnect between browsers (WebSocket) and TCP
//! clients, and stores the shared layout.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rolcb::hub::{self, Hub};
use s4d_bridge::{router, AppState};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

const T: Duration = Duration::from_secs(3);

async fn http(addr: std::net::SocketAddr, req: String) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    timeout(T, s.read_to_string(&mut out))
        .await
        .unwrap()
        .unwrap();
    out
}

#[tokio::test]
async fn relay_and_layout() {
    let dir = std::env::temp_dir().join(format!("s4d-bridge-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let layout_path = dir.join("layout.json");
    let _ = std::fs::remove_file(&layout_path);

    let hub = Hub::new();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    tokio::spawn(hub::serve_tcp(hub.clone(), tcp));

    let state = AppState {
        hub: hub.clone(),
        layout_path: Some(layout_path.clone()),
        uplinks: vec![],
        static_dir: None,
    };
    let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web_addr = web.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(web, router(state)).await.unwrap() });

    // Browser and a JMRI-like TCP client.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{web_addr}/ws"))
        .await
        .unwrap();
    let mut jmri = BufReader::new(TcpStream::connect(tcp_addr).await.unwrap());
    timeout(T, async {
        while hub.port_count() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    ws.send(Message::text(":X195B4123N0501010122000010;\n"))
        .await
        .unwrap();
    let mut line = String::new();
    timeout(T, jmri.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(line.trim(), ":X195B4123N0501010122000010;");

    jmri.get_mut()
        .write_all(b":X19100456N050101010001;")
        .await
        .unwrap();
    let msg = timeout(T, ws.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(msg.to_text().unwrap().trim(), ":X19100456N050101010001;");

    // Info + shared layout.
    let info = http(
        web_addr,
        "GET /api/info HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".to_string(),
    )
    .await;
    assert!(info.contains("\"shared_layout\":true"), "{info}");
    let missing = http(
        web_addr,
        "GET /layout.json HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".to_string(),
    )
    .await;
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");

    let body = s4d_core::Layout::demo().to_json();
    let put = http(
        web_addr,
        format!("PUT /layout.json HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()),
    )
    .await;
    assert!(put.starts_with("HTTP/1.1 204"), "{put}");
    assert_eq!(std::fs::read_to_string(&layout_path).unwrap(), body);

    let bad = http(
        web_addr,
        "PUT /layout.json HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{x}"
            .into(),
    )
    .await;
    assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");

    let index = http(
        web_addr,
        "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(index.starts_with("HTTP/1.1 200"), "{index}");
    let traversal = http(
        web_addr,
        "GET /../Cargo.toml HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(!traversal.contains("[package]"));

    let _ = std::fs::remove_dir_all(&dir);
}
