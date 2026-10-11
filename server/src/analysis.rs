use std::io;

use shakmaty::{Color, Position, uci::UciMove};
use tokio::sync::{mpsc, watch};

use crate::uci::{
    Engine, GamePosition,
    info::{self, Bound, Info, Score},
};

/// One principal variation, scored from White's point of view.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub depth: u32,
    pub score: Score,
    pub bound: Bound,
    pub wdl: Option<[u32; 3]>,
    pub pv: Vec<UciMove>,
}

/// Latest analysis of request `generation`. `lines[i]` is MultiPV line `i + 1`.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub depth: u32,
    pub nodes: u64,
    pub nps: u64,
    pub lines: Vec<Line>,
}

impl Snapshot {
    fn reset(&mut self, generation: u64) {
        *self = Self {
            generation,
            lines: std::mem::take(&mut self.lines),
            ..Self::default()
        };
        self.lines.clear();
    }

    fn apply(&mut self, info: Info) {
        self.depth = info.depth.unwrap_or(self.depth);
        self.nodes = info.nodes.unwrap_or(self.nodes);
        self.nps = info.nps.unwrap_or(self.nps);
        let (Some(depth), Some(score)) = (info.depth, info.score) else {
            return;
        };
        if info.pv.is_empty() {
            return;
        }
        let i = info.multipv.unwrap_or(1).saturating_sub(1) as usize;
        let line = Line {
            depth,
            score,
            bound: info.bound,
            wdl: info.wdl,
            pv: info.pv,
        };
        if i < self.lines.len() {
            self.lines[i] = line;
        } else if i == self.lines.len() {
            self.lines.push(line);
        }
    }
}

struct Request {
    generation: u64,
    position: GamePosition,
    multipv: u32,
}

/// Runs infinite analysis on one engine. Only the newest request is ever searched:
/// intermediate requests are skipped and output from superseded searches is dropped.
pub struct Analyzer {
    requests: mpsc::UnboundedSender<Option<Request>>,
    snapshot: watch::Receiver<Snapshot>,
}

impl Analyzer {
    /// Takes an engine that has completed its handshake and options.
    pub fn new(engine: Engine) -> Self {
        let (requests, rx) = mpsc::unbounded_channel();
        let (tx, snapshot) = watch::channel(Snapshot::default());
        tokio::spawn(async move {
            if let Err(e) = run(engine, rx, tx).await {
                tracing::error!("analysis engine stopped: {e}");
            }
        });
        Self { requests, snapshot }
    }

    pub fn analyze(&self, generation: u64, position: GamePosition, multipv: u32) {
        let _ = self.requests.send(Some(Request {
            generation,
            position,
            multipv: multipv.max(1),
        }));
    }

    pub fn stop(&self) {
        let _ = self.requests.send(None);
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.snapshot.clone()
    }
}

async fn run(
    mut engine: Engine,
    mut requests: mpsc::UnboundedReceiver<Option<Request>>,
    snapshot: watch::Sender<Snapshot>,
) -> io::Result<()> {
    let mut multipv = 1;
    let mut turn = Color::White;
    let mut searching = false;
    let mut stopping = false;
    let mut pending = None;
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return Ok(()) };
                pending = request;
                if searching && !stopping {
                    engine.send("stop").await?;
                    stopping = true;
                }
            }
            line = engine.recv() => {
                let line = line.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "engine exited"))?;
                if line.starts_with("bestmove") {
                    searching = false;
                    stopping = false;
                } else if searching && !stopping && let Some(info) = info::parse(&line) {
                    snapshot.send_modify(|s| s.apply(info.to_white_pov(turn)));
                }
            }
        }
        if !searching
            && let Some(Request {
                generation,
                position,
                multipv: wanted,
            }) = pending.take()
        {
            if wanted != multipv {
                engine.set_option("MultiPV", wanted).await?;
                multipv = wanted;
            }
            engine.send(position.command()).await?;
            engine.send("go infinite").await?;
            turn = position.position().turn();
            snapshot.send_modify(|s| s.reset(generation));
            searching = true;
        }
    }
}
