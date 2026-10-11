use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header, uri::Authority},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, san::SanPlus, uci::UciMove};
use tokio::time::{Instant, sleep_until};
use tower_http::services::{ServeDir, ServeFile};

mod api;
mod matches;

use crate::{
    analysis::{Analyzer, EngineOptions, Snapshot},
    arena::Arena,
    engines::Registry,
    uci::{
        GamePosition,
        info::{Bound, Score},
    },
};

const SEND_INTERVAL: Duration = Duration::from_millis(50);
const MAX_MESSAGE: usize = 64 * 1024;
const MAX_MULTIPV: u32 = 8;

pub struct AppState {
    analyzer: Option<Arc<Analyzer>>,
    registry: Arc<Registry>,
    arena: Arc<Arena>,
    generations: AtomicU64,
}

impl AppState {
    pub fn new(analyzer: Option<Analyzer>, registry: Registry) -> Self {
        Self {
            analyzer: analyzer.map(Arc::new),
            registry: Arc::new(registry),
            arena: Arc::default(),
            generations: AtomicU64::new(0),
        }
    }
}

pub fn router(state: AppState, web_dist: &str) -> Router {
    let spa = ServeDir::new(web_dist).fallback(ServeFile::new(format!("{web_dist}/index.html")));
    Router::new()
        .route("/ws", get(ws))
        .merge(api::routes())
        .merge(matches::routes())
        .fallback_service(spa)
        .layer(middleware::from_fn(loopback_only))
        .with_state(Arc::new(state))
}

/// The server executes engine code, so only pages served from loopback may talk to it.
/// Checking Host blocks DNS rebinding; checking Origin blocks cross-site requests.
async fn loopback_only(request: Request, next: Next) -> Response {
    if is_allowed(request.headers()) {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn is_allowed(headers: &HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok()?.parse::<Authority>().ok());
    let origin = headers
        .get(header::ORIGIN)
        .map(|o| o.to_str().ok()?.parse::<Uri>().ok()?.authority().cloned());
    host.is_some_and(|a| is_loopback(a.host()))
        && origin.is_none_or(|a| a.is_some_and(|a| is_loopback(a.host())))
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ClientMessage {
    Analyze {
        #[serde(rename = "gen")]
        generation: u64,
        root_fen: String,
        moves: Vec<String>,
        multi_pv: u32,
    },
    Stop,
    Options {
        threads: usize,
        hash_mb: usize,
    },
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ServerMessage<'a> {
    Analysis {
        #[serde(rename = "gen")]
        generation: u64,
        depth: u32,
        nodes: u64,
        nps: u64,
        lines: Vec<LineOut<'a>>,
    },
    Error {
        message: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LineOut<'a> {
    multi_pv: usize,
    depth: u32,
    #[serde(flatten)]
    score: Score,
    #[serde(skip_serializing_if = "is_exact")]
    bound: Bound,
    #[serde(skip_serializing_if = "Option::is_none")]
    wdl: Option<[u32; 3]>,
    pv_uci: &'a [String],
    pv_san: &'a [String],
}

fn is_exact(bound: &Bound) -> bool {
    *bound == Bound::Exact
}

async fn ws(upgrade: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    upgrade
        .max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| session(socket, state))
}

/// Forwards this connection's analysis at most every `SEND_INTERVAL`, sending the
/// first update after a quiet period immediately.
async fn session(mut socket: WebSocket, state: Arc<AppState>) {
    let Some(analyzer) = &state.analyzer else {
        let _ = send_error(&mut socket, "Stockfish is not available".into()).await;
        return;
    };
    let mut snapshots = analyzer.subscribe();
    let mut view: Option<View> = None;
    let mut dirty = false;
    let mut next_send = Instant::now();
    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                let Message::Text(text) = message else { continue };
                if let Err(e) = handle(&text, analyzer, &state.generations, &mut view)
                    && send_error(&mut socket, e).await.is_err()
                {
                    break;
                }
            }
            changed = snapshots.changed() => {
                if changed.is_err() {
                    let _ = send_error(&mut socket, "analysis engine stopped".into()).await;
                    break;
                }
                dirty = true;
            }
            () = sleep_until(next_send), if dirty => {
                dirty = false;
                let json = view.as_mut().and_then(|v| v.render(&snapshots.borrow_and_update()));
                if let Some(json) = json {
                    if socket.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                    next_send = Instant::now() + SEND_INTERVAL;
                }
            }
        }
    }
    if let Some(view) = view {
        analyzer.stop(view.generation);
    }
}

fn handle(
    text: &str,
    analyzer: &Analyzer,
    generations: &AtomicU64,
    view: &mut Option<View>,
) -> Result<(), String> {
    match serde_json::from_str(text).map_err(|e| e.to_string())? {
        ClientMessage::Analyze {
            generation,
            root_fen,
            moves,
            multi_pv,
        } => {
            let position = GamePosition::new(&root_fen, &moves).map_err(|e| e.to_string())?;
            // Server-wide generations keep clients from seeing each other's analysis.
            let internal = generations.fetch_add(1, Ordering::Relaxed) + 1;
            *view = Some(View::new(internal, generation, position.position().clone()));
            analyzer.analyze(internal, position, multi_pv.clamp(1, MAX_MULTIPV));
        }
        ClientMessage::Stop => {
            if let Some(view) = view.take() {
                analyzer.stop(view.generation);
            }
        }
        ClientMessage::Options { threads, hash_mb } => {
            analyzer.configure(EngineOptions { threads, hash_mb }.clamped());
        }
    }
    Ok(())
}

async fn send_error(socket: &mut WebSocket, message: String) -> Result<(), axum::Error> {
    let json = serde_json::to_string(&ServerMessage::Error { message }).expect("serializable");
    socket.send(Message::Text(json.into())).await
}

/// The position a connection is analyzing, with SAN cached per PV.
struct View {
    generation: u64,
    client_generation: u64,
    position: Chess,
    pvs: Vec<Pv>,
}

#[derive(Default)]
struct Pv {
    moves: Vec<UciMove>,
    uci: Vec<String>,
    san: Vec<String>,
}

impl View {
    fn new(generation: u64, client_generation: u64, position: Chess) -> Self {
        Self {
            generation,
            client_generation,
            position,
            pvs: Vec::new(),
        }
    }

    fn render(&mut self, snapshot: &Snapshot) -> Option<String> {
        if snapshot.generation != self.generation || snapshot.lines.is_empty() {
            return None;
        }
        self.pvs.resize_with(snapshot.lines.len(), Pv::default);
        for (line, pv) in snapshot.lines.iter().zip(&mut self.pvs) {
            if pv.moves != line.pv {
                *pv = Pv::new(&self.position, &line.pv);
            }
        }
        let lines = snapshot
            .lines
            .iter()
            .zip(&self.pvs)
            .enumerate()
            .map(|(i, (line, pv))| LineOut {
                multi_pv: i + 1,
                depth: line.depth,
                score: line.score,
                bound: line.bound,
                wdl: line.wdl,
                pv_uci: &pv.uci,
                pv_san: &pv.san,
            })
            .collect();
        let message = ServerMessage::Analysis {
            generation: self.client_generation,
            depth: snapshot.depth,
            nodes: snapshot.nodes,
            nps: snapshot.nps,
            lines,
        };
        Some(serde_json::to_string(&message).expect("serializable"))
    }
}

impl Pv {
    fn new(position: &Chess, moves: &[UciMove]) -> Self {
        let mut position = position.clone();
        let mut pv = Self {
            moves: moves.to_vec(),
            uci: Vec::with_capacity(moves.len()),
            san: Vec::with_capacity(moves.len()),
        };
        for &uci in moves {
            let Ok(m) = uci.to_move(&position) else { break };
            pv.uci.push(uci.to_string());
            pv.san
                .push(SanPlus::from_move_and_play_unchecked(&mut position, m).to_string());
        }
        pv
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::analysis::Line;

    fn headers(host: Option<&'static str>, origin: Option<&'static str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(host) = host {
            headers.insert(header::HOST, HeaderValue::from_static(host));
        }
        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
        }
        headers
    }

    #[test]
    fn loopback_guard() {
        for (host, origin) in [
            (Some("localhost:7878"), None),
            (Some("127.0.0.1:7878"), Some("http://localhost:5173")),
            (Some("localhost:5174"), Some("http://localhost:5174")),
            (Some("[::1]:7878"), Some("http://[::1]:7878")),
        ] {
            assert!(is_allowed(&headers(host, origin)), "{host:?} {origin:?}");
        }
        for (host, origin) in [
            (None, None),
            (Some("evil.example:7878"), None),
            (Some("localhost.evil.example"), None),
            (Some("localhost:7878"), Some("http://evil.example")),
            (
                Some("localhost:7878"),
                Some("http://localhost.evil.example:7878"),
            ),
            (Some("localhost:7878"), Some("null")),
        ] {
            assert!(!is_allowed(&headers(host, origin)), "{host:?} {origin:?}");
        }
    }

    #[test]
    fn render_converts_pv_to_san_and_filters_generation() {
        let position = GamePosition::new(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &["e2e4"],
        )
        .unwrap();
        let mut view = View::new(7, 3, position.position().clone());
        let snapshot = Snapshot {
            generation: 7,
            depth: 10,
            nodes: 1,
            nps: 2,
            lines: vec![Line {
                depth: 10,
                score: Score::Cp(30),
                bound: Bound::Exact,
                wdl: Some([1, 2, 3]),
                pv: vec!["e7e5".parse().unwrap(), "g1f3".parse().unwrap()],
            }],
        };
        assert_eq!(
            view.render(&snapshot).unwrap(),
            r#"{"type":"analysis","gen":3,"depth":10,"nodes":1,"nps":2,"lines":[{"multiPv":1,"depth":10,"cp":30,"wdl":[1,2,3],"pvUci":["e7e5","g1f3"],"pvSan":["e5","Nf3"]}]}"#
        );
        assert!(
            view.render(&Snapshot {
                generation: 6,
                ..snapshot.clone()
            })
            .is_none()
        );
        assert!(
            view.render(&Snapshot {
                lines: Vec::new(),
                ..snapshot
            })
            .is_none()
        );
    }

    #[test]
    fn client_messages() {
        let ClientMessage::Analyze { generation, root_fen, moves, multi_pv } = serde_json::from_str(
            r#"{"type":"analyze","gen":4,"rootFen":"8/8/8/8/8/8/8/8 w - - 0 1","moves":["e2e4"],"multiPv":3}"#,
        )
        .unwrap() else {
            panic!("expected analyze");
        };
        assert_eq!(
            (generation, root_fen.len(), moves.len(), multi_pv),
            (4, 25, 1, 3)
        );
        assert!(matches!(
            serde_json::from_str(r#"{"type":"stop"}"#).unwrap(),
            ClientMessage::Stop
        ));
        assert!(matches!(
            serde_json::from_str(r#"{"type":"options","threads":4,"hashMb":512}"#).unwrap(),
            ClientMessage::Options {
                threads: 4,
                hash_mb: 512
            }
        ));
    }
}
