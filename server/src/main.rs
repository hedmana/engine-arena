use std::{
    env, io,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use engine_arena::{
    analysis::{Analyzer, EngineOptions},
    uci::Engine,
    web::{self, AppState},
};

const PORT: u16 = 7878;
const WEB_DIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist");

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let analyzer = start_analyzer()
        .await
        .inspect_err(|e| tracing::warn!("analysis disabled: {e}"))
        .ok();
    let app = web::router(AppState::new(analyzer), WEB_DIST);

    // Loopback only: this server executes engine code.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tracing::info!("listening on http://{addr}");
    // Returning from main drops every task, and with them the engines' process groups.
    tokio::select! {
        result = axum::serve(listener, app) => result.expect("serve"),
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }
}

async fn start_analyzer() -> io::Result<Analyzer> {
    let stockfish = env::var("STOCKFISH").unwrap_or_else(|_| "stockfish".into());
    let mut engine = Engine::start(&stockfish, &[], Duration::from_secs(10)).await?;
    engine.set_option("UCI_ShowWDL", true).await?;
    tracing::info!("analysis engine: {}", engine.name);
    let analyzer = Analyzer::new(engine);
    analyzer.configure(EngineOptions::default());
    Ok(analyzer)
}
