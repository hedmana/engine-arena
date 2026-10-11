use std::{
    io::{self, BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::Duration,
};

use engine_arena::{
    analysis::{Analyzer, EngineOptions, Snapshot},
    uci::{Engine, GamePosition, info},
};
use shakmaty::{CastlingMode, Chess, Position, uci::UciMove};
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const SECOND: Duration = Duration::from_secs(1);

async fn stockfish(threads: usize, hash_mb: usize) -> Option<Engine> {
    let mut engine = match Engine::start("stockfish", &[], None, 5 * SECOND).await {
        Ok(engine) => engine,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("stockfish not on PATH, skipping");
            return None;
        }
        Err(e) => panic!("{e}"),
    };
    engine.set_option("Threads", threads).await.unwrap();
    engine.set_option("Hash", hash_mb).await.unwrap();
    engine.ready(5 * SECOND).await.unwrap();
    Some(engine)
}

/// Moves of a deterministic pseudo-random game, like scrolling through a game record.
fn walk(plies: usize) -> Vec<String> {
    let mut position = Chess::default();
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    (0..plies)
        .map(|_| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let legal = position.legal_moves();
            let m = legal[(seed >> 33) as usize % legal.len()];
            position.play_unchecked(m);
            m.to_uci(CastlingMode::Standard).to_string()
        })
        .collect()
}

fn pv_is_legal(position: &Chess, pv: &[UciMove]) -> bool {
    let mut position = position.clone();
    pv.iter().all(|uci| {
        uci.to_move(&position)
            .map(|m| position.play_unchecked(m))
            .is_ok()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn rapid_switches_analyze_only_the_last_position() {
    let Some(engine) = stockfish(2, 64).await else {
        return;
    };
    let analyzer = Analyzer::new(engine);
    let mut snapshots = analyzer.subscribe();
    let moves = walk(50);
    let positions: Vec<GamePosition> = (1..=moves.len())
        .map(|n| GamePosition::new(START, &moves[..n]).unwrap())
        .collect();
    let finals: Vec<Chess> = positions.iter().map(|p| p.position().clone()).collect();
    let last = finals.len() as u64 - 1;

    for (generation, position) in positions.into_iter().enumerate() {
        analyzer.analyze(generation as u64, position, 3);
        sleep(Duration::from_millis(2)).await;
    }

    timeout(10 * SECOND, async {
        loop {
            snapshots.changed().await.expect("analyzer alive");
            let s = snapshots.borrow_and_update().clone();
            for line in &s.lines {
                assert!(
                    pv_is_legal(&finals[s.generation as usize], &line.pv),
                    "stale pv in generation {}",
                    s.generation
                );
            }
            if s.generation == last && s.depth >= 12 && !s.lines.is_empty() {
                return;
            }
        }
    })
    .await
    .expect("deep analysis of the last position");
}

async fn changes_within(snapshots: &mut watch::Receiver<Snapshot>, within: Duration) -> bool {
    timeout(within, snapshots.changed()).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_is_generation_scoped_and_configure_resumes() {
    let Some(engine) = stockfish(2, 64).await else {
        return;
    };
    let analyzer = Analyzer::new(engine);
    let mut snapshots = analyzer.subscribe();
    let quiet = Duration::from_millis(300);

    analyzer.analyze(1, GamePosition::new(START, &[] as &[&str]).unwrap(), 1);
    analyzer.stop(0);
    assert!(
        changes_within(&mut snapshots, SECOND).await,
        "stale stop must not stop generation 1"
    );

    analyzer.stop(1);
    sleep(Duration::from_millis(50)).await;
    snapshots.borrow_and_update();
    assert!(
        !changes_within(&mut snapshots, quiet).await,
        "stop(1) must stop generation 1"
    );

    analyzer.configure(EngineOptions {
        threads: 1,
        hash_mb: 16,
    });
    assert!(
        !changes_within(&mut snapshots, quiet).await,
        "configure must not resume a stopped search"
    );

    analyzer.analyze(2, GamePosition::new(START, &["e2e4"]).unwrap(), 1);
    assert!(changes_within(&mut snapshots, SECOND).await);
    analyzer.configure(EngineOptions {
        threads: 2,
        hash_mb: 32,
    });
    sleep(Duration::from_millis(100)).await;
    snapshots.borrow_and_update();
    assert!(
        changes_within(&mut snapshots, SECOND).await,
        "configure must resume generation 2"
    );
    assert_eq!(snapshots.borrow().generation, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_stops_search_and_resume_continues() {
    let Some(engine) = stockfish(2, 64).await else {
        return;
    };
    let analyzer = Analyzer::new(engine);
    let mut snapshots = analyzer.subscribe();
    analyzer.analyze(1, GamePosition::new(START, &[] as &[&str]).unwrap(), 1);
    assert!(changes_within(&mut snapshots, SECOND).await);

    analyzer.pause(true);
    analyzer.analyze(2, GamePosition::new(START, &["d2d4"]).unwrap(), 1);
    sleep(Duration::from_millis(50)).await;
    snapshots.borrow_and_update();
    assert!(
        !changes_within(&mut snapshots, Duration::from_millis(300)).await,
        "paused analyzer must not search"
    );

    analyzer.pause(false);
    assert!(
        changes_within(&mut snapshots, SECOND).await,
        "resume must start the newest request"
    );
    assert_eq!(snapshots.borrow().generation, 2);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark: cargo test --release -- --ignored --nocapture"]
async fn nps_matches_standalone_stockfish() {
    const SEARCH: Duration = Duration::from_secs(5);
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .saturating_sub(2)
        .max(1);
    let hash_mb = 1024;

    let mut child = Command::new("stockfish")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        child.stdin.as_mut().unwrap(),
        "setoption name Threads value {threads}\nsetoption name Hash value {hash_mb}\nisready\nposition startpos\ngo movetime {}\n",
        SEARCH.as_millis()
    )
    .unwrap();
    let standalone = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .take_while(|line| !line.starts_with("bestmove"))
        .filter_map(|line| info::parse(&line)?.nps)
        .last()
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    let analyzer = Analyzer::new(stockfish(threads, hash_mb).await.unwrap());
    analyzer.analyze(0, GamePosition::new(START, &[] as &[&str]).unwrap(), 1);
    sleep(SEARCH).await;
    let driver = analyzer.subscribe().borrow().nps;

    eprintln!("threads {threads}: standalone {standalone} nps, driver {driver} nps");
    assert!(
        driver as f64 >= standalone as f64 * 0.98,
        "driver {driver} < 98% of standalone {standalone}"
    );
}
