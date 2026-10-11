use std::net::{Ipv4Addr, SocketAddr};

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
    routing::get,
};
use tower_http::services::{ServeDir, ServeFile};

const PORT: u16 = 7878;
const WEB_DIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist");

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let spa = ServeDir::new(WEB_DIST).fallback(ServeFile::new(format!("{WEB_DIST}/index.html")));
    let app = Router::new().route("/ws", get(ws)).fallback_service(spa);

    // Loopback only: this server will execute uploaded engine code.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve");
}

async fn ws(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg
            && text.as_str() == "ping"
            && socket.send(Message::Text("pong".into())).await.is_err()
        {
            break;
        }
    }
}
