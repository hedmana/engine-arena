use std::{
    env, io,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};

use engine_arena::{
    analysis::{Analyzer, EngineOptions},
    engines::Registry,
    uci::Engine,
    web::{self, AppState},
};

const PORT: u16 = 7878;
const WEB_DIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist");
const ENGINES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/engines");

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let stockfish = env::var("STOCKFISH").unwrap_or_else(|_| "stockfish".into());
    let analyzer = start_analyzer(&stockfish)
        .await
        .inspect_err(|e| tracing::warn!("analysis disabled: {e}"))
        .ok();
    let registry = Registry::open(Path::new(ENGINES_DIR), stockfish)
        .await
        .expect("open engine registry");
    let app = web::router(AppState::new(analyzer, registry), WEB_DIST);

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

async fn start_analyzer(stockfish: &str) -> io::Result<Analyzer> {
    let mut engine = Engine::start(stockfish, &[], None, Duration::from_secs(10)).await?;
    engine.set_option("UCI_ShowWDL", true).await?;
    tracing::info!("analysis engine: {}", engine.name);
    let analyzer = Analyzer::new(engine);
    analyzer.configure(EngineOptions::default());
    Ok(analyzer)
}
