mod pgn;

use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Position, fen::Fen, san::SanPlus, uci::UciMove,
    zobrist::Zobrist64,
};
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::broadcast,
    task::JoinHandle,
    time::{Instant, timeout},
};

use crate::{
    analysis::Analyzer,
    engines::{EngineEntry, Registry},
    uci::{
        Engine,
        info::{self, Bound, Score},
    },
};

const OPENINGS: &str = include_str!("arena/openings.txt");
const STARTUP: Duration = Duration::from_secs(60);
const NEW_GAME: Duration = Duration::from_secs(10);
const MEMORY_LIMIT: u64 = 1 << 30;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchConfig {
    pub engines: [String; 2],
    pub base_ms: u64,
    #[serde(default)]
    pub increment_ms: u64,
    pub games: u32,
    #[serde(default = "default_max_plies")]
    pub max_plies: u32,
    /// Tolerance for process and pipe latency before a late move loses on time.
    #[serde(default = "default_overhead_ms")]
    pub overhead_ms: u64,
}

fn default_max_plies() -> u32 {
    400
}

fn default_overhead_ms() -> u64 {
    50
}

impl MatchConfig {
    fn validate(&mut self) -> Result<(), String> {
        let rules = [
            ((1..=1000).contains(&self.games), "games must be 1 to 1000"),
            (
                (100..=3_600_000).contains(&self.base_ms),
                "base time must be 0.1 s to 1 h",
            ),
            (
                self.increment_ms <= 60_000,
                "increment must be at most 60 s",
            ),
            (
                (20..=2000).contains(&self.max_plies),
                "max plies must be 20 to 2000",
            ),
            (self.overhead_ms <= 5000, "overhead must be at most 5 s"),
        ];
        if let Some((_, message)) = rules.iter().find(|(ok, _)| !ok) {
            return Err((*message).into());
        }
        // Each opening is played twice with colors swapped.
        self.games += self.games % 2;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    pub base_ms: u64,
    pub increment_ms: u64,
    pub overhead_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GameResult {
    #[serde(rename = "1-0")]
    WhiteWins,
    #[serde(rename = "0-1")]
    BlackWins,
    #[serde(rename = "1/2-1/2")]
    Draw,
}

impl GameResult {
    fn win(color: Color) -> Self {
        if color == Color::White {
            Self::WhiteWins
        } else {
            Self::BlackWins
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WhiteWins => "1-0",
            Self::BlackWins => "0-1",
            Self::Draw => "1/2-1/2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    Checkmate,
    Stalemate,
    Repetition,
    FiftyMoves,
    InsufficientMaterial,
    MaxPlies,
    Timeout,
    IllegalMove,
    Crash,
    MemoryLimit,
    Unresponsive,
}

impl Termination {
    /// The losing engine's process can no longer be trusted (it may still be searching or is dead).
    fn needs_restart(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Crash | Self::MemoryLimit | Self::Unresponsive
        )
    }
}

/// Engine-reported score from White's point of view.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Eval {
    #[serde(flatten)]
    pub score: Score,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRecord {
    pub uci: String,
    pub san: String,
    /// Mover's remaining time after the move; `None` for opening book moves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval: Option<Eval>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRecord {
    pub number: u32,
    /// Index of the engine playing White.
    pub white: usize,
    pub moves: Vec<MoveRecord>,
    /// Remaining time of [White, Black].
    pub clocks: [u64; 2],
    pub result: Option<GameResult>,
    pub termination: Option<Termination>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Participant {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Finished,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchState {
    pub id: u64,
    /// UTC date the match started, `YYYY.MM.DD`.
    pub date: String,
    pub engines: [Participant; 2],
    pub base_ms: u64,
    pub increment_ms: u64,
    pub games: u32,
    pub max_plies: u32,
    pub status: Status,
    pub wins: [u32; 2],
    pub draws: u32,
    pub finished: Vec<GameRecord>,
    pub current: Option<GameRecord>,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum Event {
    MatchStart {
        state: MatchState,
    },
    GameStart {
        game: GameRecord,
    },
    Move {
        game: u32,
        #[serde(rename = "move")]
        record: MoveRecord,
        clocks: [u64; 2],
    },
    GameEnd {
        game: GameRecord,
        wins: [u32; 2],
        draws: u32,
    },
    MatchEnd {
        status: Status,
        wins: [u32; 2],
        draws: u32,
    },
}

/// Tracks a game for legality, draw rules, and the UCI `position` command.
pub struct Referee {
    root: String,
    position: Chess,
    moves: Vec<String>,
    /// Hashes of positions since the last irreversible move, for threefold repetition.
    history: Vec<Zobrist64>,
    max_plies: usize,
}

impl Referee {
    pub fn new(root: Chess, max_plies: u32) -> Self {
        Self {
            root: Fen::from_position(&root, EnPassantMode::Legal).to_string(),
            history: vec![root.zobrist_hash(EnPassantMode::Legal)],
            position: root,
            moves: Vec::new(),
            max_plies: max_plies as usize,
        }
    }

    pub fn turn(&self) -> Color {
        self.position.turn()
    }

    /// Plays `uci` if it is legal, returning the normalized UCI and SAN.
    pub fn play(&mut self, uci: &str) -> Option<(String, String)> {
        let m = uci.parse::<UciMove>().ok()?.to_move(&self.position).ok()?;
        if self.position.is_irreversible(m) {
            self.history.clear();
        }
        let uci = m.to_uci(CastlingMode::Standard).to_string();
        let san = SanPlus::from_move_and_play_unchecked(&mut self.position, m).to_string();
        self.history
            .push(self.position.zobrist_hash(EnPassantMode::Legal));
        self.moves.push(uci.clone());
        Some((uci, san))
    }

    pub fn position_command(&self) -> String {
        if self.moves.is_empty() {
            format!("position fen {}", self.root)
        } else {
            format!("position fen {} moves {}", self.root, self.moves.join(" "))
        }
    }

    pub fn outcome(&self) -> Option<(GameResult, Termination)> {
        let p = &self.position;
        let draw = |termination| Some((GameResult::Draw, termination));
        if p.is_checkmate() {
            return Some((GameResult::win(!p.turn()), Termination::Checkmate));
        }
        if p.is_stalemate() {
            return draw(Termination::Stalemate);
        }
        if p.is_insufficient_material() {
            return draw(Termination::InsufficientMaterial);
        }
        let current = self.history.last()?;
        if self.history.iter().filter(|&h| h == current).count() >= 3 {
            return draw(Termination::Repetition);
        }
        if p.halfmoves() >= 100 {
            return draw(Termination::FiftyMoves);
        }
        if self.moves.len() >= self.max_plies {
            return draw(Termination::MaxPlies);
        }
        None
    }
}

/// Plays until the referee or a forfeit ends the game. `engines` are [White, Black].
pub async fn play_game(
    engines: [&mut Engine; 2],
    referee: &mut Referee,
    tc: TimeControl,
    mut on_move: impl FnMut(MoveRecord, [u64; 2]),
) -> (GameResult, Termination) {
    let [white, black] = engines;
    let mut clocks = [tc.base_ms; 2];
    loop {
        if let Some(outcome) = referee.outcome() {
            return outcome;
        }
        let turn = referee.turn();
        let side = usize::from(turn == Color::Black);
        let engine = if turn == Color::White {
            &mut *white
        } else {
            &mut *black
        };
        let forfeit = |termination| (GameResult::win(!turn), termination);
        let go = format!(
            "go wtime {} btime {} winc {} binc {}",
            clocks[0], clocks[1], tc.increment_ms, tc.increment_ms
        );
        if engine.send(&referee.position_command()).await.is_err()
            || engine.send(&go).await.is_err()
        {
            return forfeit(died(engine));
        }
        let started = Instant::now();
        let allowed = Duration::from_millis(clocks[side] + tc.overhead_ms);
        let (uci, eval) = match timeout(allowed, bestmove(engine, turn)).await {
            Err(_) => return forfeit(Termination::Timeout),
            Ok(None) => return forfeit(died(engine)),
            Ok(Some(reply)) => reply,
        };
        let elapsed = started.elapsed();
        if elapsed > allowed {
            return forfeit(Termination::Timeout);
        }
        clocks[side] = clocks[side].saturating_sub(elapsed.as_millis() as u64) + tc.increment_ms;
        let Some((uci, san)) = referee.play(&uci) else {
            return forfeit(Termination::IllegalMove);
        };
        on_move(
            MoveRecord {
                uci,
                san,
                clock_ms: Some(clocks[side]),
                eval,
            },
            clocks,
        );
    }
}

fn died(engine: &Engine) -> Termination {
    if engine.exceeded_memory() {
        Termination::MemoryLimit
    } else {
        Termination::Crash
    }
}

/// Waits for `bestmove`, keeping the last exact score the engine reported.
async fn bestmove(engine: &mut Engine, turn: Color) -> Option<(String, Option<Eval>)> {
    let mut eval = None;
    loop {
        let line = engine.recv().await?;
        if let Some(reply) = line.strip_prefix("bestmove") {
            return Some((
                reply
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                eval,
            ));
        }
        if let Some(info) = info::parse(&line).map(|i| i.to_white_pov(turn))
            && info.bound == Bound::Exact
            && let (Some(score), Some(depth)) = (info.score, info.depth)
        {
            eval = Some(Eval { score, depth });
        }
    }
}

#[derive(Debug)]
pub enum ArenaError {
    Busy,
    Invalid(String),
}

/// Runs one match at a time, broadcasts its progress as JSON events, and appends
/// every finished game to `<dir>/<match id>.pgn`.
pub struct Arena {
    state: Mutex<Option<MatchState>>,
    events: broadcast::Sender<Arc<str>>,
    task: Mutex<Option<JoinHandle<()>>>,
    ids: AtomicU64,
    openings: Vec<Vec<String>>,
    dir: PathBuf,
}

impl Arena {
    /// Continues match ids after the highest saved match so PGN files are never overwritten.
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let last = std::fs::read_dir(&dir)?
            .filter_map(|e| {
                e.ok()?
                    .file_name()
                    .to_str()?
                    .strip_suffix(".pgn")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        let openings = OPENINGS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split_whitespace().map(Into::into).collect())
            .collect();
        Ok(Self {
            state: Mutex::default(),
            events: broadcast::channel(1024).0,
            task: Mutex::default(),
            ids: AtomicU64::new(last),
            openings,
            dir,
        })
    }

    pub fn pgn_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{id}.pgn"))
    }

    pub fn current(&self) -> Option<MatchState> {
        self.state.lock().expect("arena state").clone()
    }

    /// The current state as a `state` event plus a receiver for every later event.
    pub fn subscribe(&self) -> (String, broadcast::Receiver<Arc<str>>) {
        let state = self.state.lock().expect("arena state");
        let snapshot = serde_json::json!({ "type": "state", "state": &*state }).to_string();
        (snapshot, self.events.subscribe())
    }

    pub async fn start(
        self: &Arc<Self>,
        mut config: MatchConfig,
        registry: Arc<Registry>,
        analyzer: Option<Arc<Analyzer>>,
    ) -> Result<u64, ArenaError> {
        config.validate().map_err(ArenaError::Invalid)?;
        let mut entries = Vec::with_capacity(2);
        for id in &config.engines {
            entries.push(
                registry
                    .get(id)
                    .await
                    .map_err(|_| ArenaError::Invalid(format!("unknown engine {id:?}")))?,
            );
        }
        let entries: [EngineEntry; 2] = entries.try_into().expect("two engines");

        let mut task = self.task.lock().expect("arena task");
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return Err(ArenaError::Busy);
        }
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        let state = MatchState {
            id,
            date: pgn::today(),
            engines: entries.clone().map(|e| Participant {
                id: e.id,
                name: e.name,
            }),
            base_ms: config.base_ms,
            increment_ms: config.increment_ms,
            games: config.games,
            max_plies: config.max_plies,
            status: Status::Running,
            wins: [0; 2],
            draws: 0,
            finished: Vec::new(),
            current: None,
        };
        *self.state.lock().expect("arena state") = Some(state.clone());
        self.emit(|_| Event::MatchStart { state });
        *task = Some(tokio::spawn(
            self.clone().run(config, entries, registry, analyzer),
        ));
        Ok(id)
    }

    /// Aborts the running match; dropping its task kills the engines.
    pub fn cancel(&self) -> bool {
        let Some(task) = self
            .task
            .lock()
            .expect("arena task")
            .take()
            .filter(|t| !t.is_finished())
        else {
            return false;
        };
        task.abort();
        let mut state = self.state.lock().expect("arena state");
        if let Some(s) = state.as_mut().filter(|s| s.status == Status::Running) {
            s.status = Status::Cancelled;
            s.current = None;
            let event = Event::MatchEnd {
                status: s.status,
                wins: s.wins,
                draws: s.draws,
            };
            let _ = self.events.send(
                serde_json::to_string(&event)
                    .expect("events serialize")
                    .into(),
            );
        }
        true
    }

    /// Applies an update to the running match and broadcasts its event; no-op once cancelled.
    fn emit(&self, update: impl FnOnce(&mut MatchState) -> Event) {
        let mut state = self.state.lock().expect("arena state");
        let Some(state) = state.as_mut().filter(|s| s.status == Status::Running) else {
            return;
        };
        let json = serde_json::to_string(&update(state)).expect("events serialize");
        let _ = self.events.send(json.into());
    }

    async fn run(
        self: Arc<Self>,
        config: MatchConfig,
        entries: [EngineEntry; 2],
        registry: Arc<Registry>,
        analyzer: Option<Arc<Analyzer>>,
    ) {
        let _paused = PausedAnalysis::new(analyzer);
        let tc = TimeControl {
            base_ms: config.base_ms,
            increment_ms: config.increment_ms,
            overhead_ms: config.overhead_ms,
        };
        let mut engines: [Option<Engine>; 2] = [None, None];
        for number in 1..=config.games {
            let white = ((number - 1) % 2) as usize;
            let opening = &self.openings[((number - 1) / 2) as usize % self.openings.len()];
            let mut referee = Referee::new(Chess::default(), config.max_plies);
            let moves = opening
                .iter()
                .map(|uci| {
                    let (uci, san) = referee.play(uci).expect("bundled openings are legal");
                    MoveRecord {
                        uci,
                        san,
                        clock_ms: None,
                        eval: None,
                    }
                })
                .collect();
            let game = GameRecord {
                number,
                white,
                moves,
                clocks: [tc.base_ms; 2],
                result: None,
                termination: None,
            };
            self.emit(|s| {
                s.current = Some(game.clone());
                Event::GameStart { game }
            });

            let seats = [white, 1 - white];
            let mut unresponsive = None;
            for (color, &i) in seats.iter().enumerate() {
                if prepare(&mut engines[i], &registry, &entries[i])
                    .await
                    .is_err()
                {
                    unresponsive = Some(Color::from_white(color == 0));
                    break;
                }
            }
            let (result, termination) = match unresponsive {
                Some(color) => (GameResult::win(!color), Termination::Unresponsive),
                None => {
                    let [a, b] = &mut engines;
                    let (a, b) = (a.as_mut().expect("prepared"), b.as_mut().expect("prepared"));
                    let players = if white == 0 { [a, b] } else { [b, a] };
                    play_game(players, &mut referee, tc, |record, clocks| {
                        self.emit(|s| {
                            if let Some(game) = &mut s.current {
                                game.moves.push(record.clone());
                                game.clocks = clocks;
                            }
                            Event::Move {
                                game: number,
                                record,
                                clocks,
                            }
                        })
                    })
                    .await
                }
            };
            if termination.needs_restart() {
                let loser = if result == GameResult::WhiteWins {
                    1 - white
                } else {
                    white
                };
                engines[loser] = None;
            }
            let mut finished_pgn: Option<(u64, String)> = None;
            self.emit(|s| {
                match result {
                    GameResult::WhiteWins => s.wins[white] += 1,
                    GameResult::BlackWins => s.wins[1 - white] += 1,
                    GameResult::Draw => s.draws += 1,
                }
                let mut game = s.current.take().expect("game in progress");
                (game.result, game.termination) = (Some(result), Some(termination));
                finished_pgn = Some((s.id, pgn::game(s, &game)));
                s.finished.push(game.clone());
                Event::GameEnd {
                    game,
                    wins: s.wins,
                    draws: s.draws,
                }
            });
            if let Some((id, pgn)) = finished_pgn
                && let Err(e) = append(&self.pgn_path(id), &pgn).await
            {
                tracing::error!("saving PGN: {e}");
            }
        }
        self.emit(|s| {
            s.status = Status::Finished;
            Event::MatchEnd {
                status: s.status,
                wins: s.wins,
                draws: s.draws,
            }
        });
    }
}

async fn append(path: &std::path::Path, text: &str) -> io::Result<()> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?
        .write_all(text.as_bytes())
        .await
}

/// Starts the engine if needed and readies it for a new game; on failure it is dropped (killed).
async fn prepare(
    slot: &mut Option<Engine>,
    registry: &Registry,
    entry: &EngineEntry,
) -> io::Result<()> {
    if slot.is_none() {
        let mut engine = registry.start(entry, STARTUP).await?;
        engine.limit_memory(MEMORY_LIMIT);
        *slot = Some(engine);
    }
    let engine = slot.as_mut().expect("started above");
    let ready = async {
        engine.send("ucinewgame").await?;
        engine.ready(NEW_GAME).await
    }
    .await;
    if ready.is_err() {
        *slot = None;
    }
    ready
}

/// Pauses analysis for the lifetime of the match task, including when it is aborted.
struct PausedAnalysis(Option<Arc<Analyzer>>);

impl PausedAnalysis {
    fn new(analyzer: Option<Arc<Analyzer>>) -> Self {
        if let Some(analyzer) = &analyzer {
            analyzer.pause(true);
        }
        Self(analyzer)
    }
}

impl Drop for PausedAnalysis {
    fn drop(&mut self) {
        if let Some(analyzer) = &self.0 {
            analyzer.pause(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn referee(fen: &str, moves: &[&str], max_plies: u32) -> Referee {
        let root = fen
            .parse::<Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap();
        let mut referee = Referee::new(root, max_plies);
        for uci in moves {
            assert!(referee.play(uci).is_some(), "{uci}");
        }
        referee
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn openings_are_legal() {
        let arena = Arena::new(std::env::temp_dir().join("engine-arena-openings-test")).unwrap();
        assert_eq!(arena.openings.len(), 40);
        for opening in &arena.openings {
            let mut referee = Referee::new(Chess::default(), 400);
            assert!(
                opening.iter().all(|uci| referee.play(uci).is_some()),
                "{opening:?}"
            );
            assert_eq!(referee.outcome(), None);
        }
    }

    #[test]
    fn outcomes() {
        let check = |fen: &str, moves: &[&str], max: u32, expected| {
            assert_eq!(referee(fen, moves, max).outcome(), expected, "{moves:?}")
        };
        check(
            START,
            &["f2f3", "e7e5", "g2g4", "d8h4"],
            400,
            Some((GameResult::BlackWins, Termination::Checkmate)),
        );
        check(
            "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
            &[],
            400,
            Some((GameResult::Draw, Termination::Stalemate)),
        );
        check(
            "8/8/8/8/8/5k2/8/5KB1 w - - 0 1",
            &[],
            400,
            Some((GameResult::Draw, Termination::InsufficientMaterial)),
        );
        check(
            "8/8/8/4k3/8/8/4K3/7R w - - 99 80",
            &["h1h2"],
            400,
            Some((GameResult::Draw, Termination::FiftyMoves)),
        );
        check("8/8/8/4k3/8/8/4K3/7R w - - 98 80", &["h1h2"], 400, None);
        check(
            START,
            &["e2e4", "e7e5", "g1f3", "b8c6"],
            4,
            Some((GameResult::Draw, Termination::MaxPlies)),
        );
        let shuffle = [
            "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
        ];
        check(START, &shuffle[..7], 400, None);
        check(
            START,
            &shuffle,
            400,
            Some((GameResult::Draw, Termination::Repetition)),
        );
    }

    #[test]
    fn rejects_illegal_moves_and_builds_commands() {
        let mut referee = referee(START, &["e1g1"; 0], 400);
        assert_eq!(referee.play("e2e5"), None);
        assert_eq!(referee.play("zz"), None);
        assert_eq!(referee.play("0000"), None);
        assert_eq!(referee.play("e2e4"), Some(("e2e4".into(), "e4".into())));
        assert_eq!(
            referee.position_command(),
            format!("position fen {START} moves e2e4")
        );
    }

    async fn fake(on_go: &str) -> Engine {
        let script = format!(
            "while read -r l; do case \"$l\" in uci) echo uciok;; isready) echo readyok;; go*) {on_go};; esac; done"
        );
        Engine::start("sh", &["-c".into(), script], None, Duration::from_secs(5))
            .await
            .unwrap()
    }

    async fn forfeit_of(mut white: Engine, base_ms: u64) -> (GameResult, Termination) {
        let mut black = fake("echo 'bestmove e7e5'").await;
        let mut referee = Referee::new(Chess::default(), 400);
        let tc = TimeControl {
            base_ms,
            increment_ms: 0,
            overhead_ms: 50,
        };
        play_game([&mut white, &mut black], &mut referee, tc, |_, _| {}).await
    }

    #[tokio::test]
    async fn forfeits() {
        let lose = |t| (GameResult::BlackWins, t);
        assert_eq!(
            forfeit_of(fake("echo 'bestmove e2e5'").await, 1000).await,
            lose(Termination::IllegalMove)
        );
        assert_eq!(
            forfeit_of(fake("sleep 5").await, 200).await,
            lose(Termination::Timeout)
        );
        assert_eq!(
            forfeit_of(fake("exit 1").await, 1000).await,
            lose(Termination::Crash)
        );

        let hog = "import sys,time\nfor l in sys.stdin:\n c=l.strip()\n if c=='uci': print('uciok',flush=True)\n elif c=='isready': print('readyok',flush=True)\n elif c.startswith('go'):\n  x=bytearray(400*1024*1024); time.sleep(10)\n";
        let mut white = Engine::start(
            "python3",
            &["-c".into(), hog.into()],
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        white.limit_memory(100 << 20);
        assert_eq!(
            forfeit_of(white, 5000).await,
            lose(Termination::MemoryLimit)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stockfish_plays_a_refereed_game() {
        let start = || Engine::start("stockfish", &[], None, Duration::from_secs(5));
        let (Ok(mut white), Ok(mut black)) = (start().await, start().await) else {
            eprintln!("stockfish not on PATH, skipping");
            return;
        };
        let mut referee = Referee::new(Chess::default(), 30);
        let tc = TimeControl {
            base_ms: 2000,
            increment_ms: 20,
            overhead_ms: 50,
        };
        let mut moves = Vec::new();
        let outcome = play_game(
            [&mut white, &mut black],
            &mut referee,
            tc,
            |record, clocks| moves.push((record, clocks)),
        )
        .await;
        assert_eq!(outcome, (GameResult::Draw, Termination::MaxPlies));
        assert_eq!(moves.len(), 30);
        assert!(
            moves
                .iter()
                .all(|(m, _)| m.eval.is_some() && m.clock_ms.is_some_and(|c| c <= 2000 + 20))
        );
    }
}
