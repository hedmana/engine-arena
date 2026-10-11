use std::{io, time::Duration};

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

#[derive(Clone)]
struct Request {
    generation: u64,
    position: GamePosition,
    multipv: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineOptions {
    pub threads: usize,
    pub hash_mb: usize,
}

impl Default for EngineOptions {
    /// Leaves two cores for the UI and the server.
    fn default() -> Self {
        Self {
            threads: cores().saturating_sub(2).max(1),
            hash_mb: 1024,
        }
    }
}

impl EngineOptions {
    pub fn clamped(self) -> Self {
        Self {
            threads: self.threads.clamp(1, cores()),
            hash_mb: self.hash_mb.clamp(16, 8192),
        }
    }
}

fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

enum Command {
    Analyze(Request),
    Stop { generation: u64 },
    Configure(EngineOptions),
}

/// Runs infinite analysis on one engine. Only the newest request is ever searched:
/// intermediate requests are skipped and output from superseded searches is dropped.
pub struct Analyzer {
    commands: mpsc::UnboundedSender<Command>,
    snapshot: watch::Receiver<Snapshot>,
}

impl Analyzer {
    /// Takes an engine that has completed its handshake.
    pub fn new(engine: Engine) -> Self {
        let (commands, rx) = mpsc::unbounded_channel();
        let (tx, snapshot) = watch::channel(Snapshot::default());
        tokio::spawn(async move {
            if let Err(e) = run(engine, rx, tx).await {
                tracing::error!("analysis engine stopped: {e}");
            }
        });
        Self { commands, snapshot }
    }

    pub fn analyze(&self, generation: u64, position: GamePosition, multipv: u32) {
        let multipv = multipv.max(1);
        let _ = self.commands.send(Command::Analyze(Request {
            generation,
            position,
            multipv,
        }));
    }

    /// Stops analysis if `generation` is still the newest request, so one client cannot stop another's.
    pub fn stop(&self, generation: u64) {
        let _ = self.commands.send(Command::Stop { generation });
    }

    /// Applies Threads and Hash between searches, then resumes the current request.
    pub fn configure(&self, options: EngineOptions) {
        let _ = self.commands.send(Command::Configure(options));
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.snapshot.clone()
    }
}

async fn run(
    mut engine: Engine,
    mut commands: mpsc::UnboundedReceiver<Command>,
    snapshot: watch::Sender<Snapshot>,
) -> io::Result<()> {
    let mut multipv = 1;
    let mut turn = Color::White;
    let mut searching = false;
    let mut stopping = false;
    let mut wanted = None;
    let mut current: Option<Request> = None;
    let mut pending: Option<Request> = None;
    let mut options = None;
    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    None => return Ok(()),
                    Some(Command::Analyze(request)) => {
                        wanted = Some(request.generation);
                        pending = Some(request);
                    }
                    Some(Command::Stop { generation }) if wanted == Some(generation) => {
                        wanted = None;
                        pending = None;
                    }
                    Some(Command::Stop { .. }) => {}
                    Some(Command::Configure(o)) => options = Some(o),
                }
                let superseded = pending.is_some() || options.is_some() || wanted.is_none();
                if searching && !stopping && superseded {
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
        if searching {
            continue;
        }
        if let Some(EngineOptions { threads, hash_mb }) = options.take() {
            engine.set_option("Threads", threads).await?;
            engine.set_option("Hash", hash_mb).await?;
            engine.ready(Duration::from_secs(30)).await?;
            if pending.is_none() && wanted.is_some() {
                pending = current.take();
            }
        }
        if let Some(request) = pending.take() {
            if request.multipv != multipv {
                engine.set_option("MultiPV", request.multipv).await?;
                multipv = request.multipv;
            }
            engine.send(request.position.command()).await?;
            engine.send("go infinite").await?;
            turn = request.position.position().turn();
            snapshot.send_modify(|s| s.reset(request.generation));
            searching = true;
            current = Some(request);
        }
    }
}
