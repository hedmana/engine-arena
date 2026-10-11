use std::{
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{GameRecord, GameResult, MatchState, MoveRecord, Termination};
use crate::uci::info::Score;

const LINE_WIDTH: usize = 80;

/// One game as PGN, with `[%clk]` and `[%eval]` comments and the ending explained.
pub fn game(state: &MatchState, game: &GameRecord) -> String {
    let result = game.result.map_or("*", GameResult::as_str);
    let white = &state.engines[game.white].name;
    let black = &state.engines[1 - game.white].name;
    let tags = [
        ("Event", format!("Engine Arena match {}", state.id)),
        ("Site", "Engine Arena".into()),
        ("Date", state.date.clone()),
        ("Round", game.number.to_string()),
        ("White", white.clone()),
        ("Black", black.clone()),
        ("Result", result.into()),
        (
            "TimeControl",
            format!("{}+{}", seconds(state.base_ms), seconds(state.increment_ms)),
        ),
        (
            "Termination",
            game.termination
                .map_or("unterminated", termination_tag)
                .into(),
        ),
    ];
    let mut pgn = String::new();
    for (tag, value) in tags {
        let value = value.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(pgn, "[{tag} \"{value}\"]").expect("writing to a String");
    }
    pgn.push('\n');

    let mut tokens = Vec::with_capacity(game.moves.len() * 3 + 2);
    for (ply, record) in game.moves.iter().enumerate() {
        if ply % 2 == 0 {
            tokens.push(format!("{}.", ply / 2 + 1));
        }
        tokens.push(record.san.clone());
        tokens.push(comment(record));
    }
    if let (Some(result), Some(termination)) = (game.result, game.termination) {
        tokens.push(format!("{{{}}}", reason(result, termination, white, black)));
    }
    tokens.push(result.into());

    let mut line = 0;
    for token in tokens {
        if line > 0 && line + 1 + token.len() > LINE_WIDTH {
            pgn.push('\n');
            line = 0;
        } else if line > 0 {
            pgn.push(' ');
            line += 1;
        }
        line += token.len();
        pgn.push_str(&token);
    }
    pgn.push_str("\n\n");
    pgn
}

fn comment(record: &MoveRecord) -> String {
    let Some(clock_ms) = record.clock_ms else {
        return "{book}".into();
    };
    let eval = record.eval.map(|e| match e.score {
        Score::Cp(cp) => format!("[%eval {:.2},{}] ", f64::from(cp) / 100.0, e.depth),
        Score::Mate(n) => format!("[%eval #{n},{}] ", e.depth),
    });
    let tenths = clock_ms / 100;
    let clock = format!(
        "{}:{:02}:{:02}.{}",
        tenths / 36_000,
        tenths / 600 % 60,
        tenths / 10 % 60,
        tenths % 10
    );
    format!("{{{}[%clk {clock}]}}", eval.unwrap_or_default())
}

fn seconds(ms: u64) -> String {
    let s = format!("{}.{:03}", ms / 1000, ms % 1000);
    s.trim_end_matches('0').trim_end_matches('.').into()
}

/// Standard PGN `Termination` values.
fn termination_tag(termination: Termination) -> &'static str {
    match termination {
        Termination::Timeout => "time forfeit",
        Termination::IllegalMove => "rules infraction",
        Termination::Crash | Termination::MemoryLimit | Termination::Unresponsive => "abandoned",
        Termination::MaxPlies => "adjudication",
        _ => "normal",
    }
}

fn reason(result: GameResult, termination: Termination, white: &str, black: &str) -> String {
    let loser = if result == GameResult::WhiteWins {
        black
    } else {
        white
    };
    match termination {
        Termination::Checkmate => format!("{loser} is checkmated"),
        Termination::Stalemate => "Draw by stalemate".into(),
        Termination::Repetition => "Draw by threefold repetition".into(),
        Termination::FiftyMoves => "Draw by the fifty-move rule".into(),
        Termination::InsufficientMaterial => "Draw by insufficient material".into(),
        Termination::MaxPlies => "Draw by move limit".into(),
        Termination::Timeout => format!("{loser} loses on time"),
        Termination::IllegalMove => format!("{loser} made an illegal move"),
        Termination::Crash => format!("{loser} crashed"),
        Termination::MemoryLimit => format!("{loser} exceeded the memory limit"),
        Termination::Unresponsive => format!("{loser} did not respond"),
    }
}

/// Today's UTC date as `YYYY.MM.DD`.
pub fn today() -> String {
    date(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400,
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn date(days_since_epoch: u64) -> String {
    let z = days_since_epoch as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{Eval, Participant, Status};

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970.01.01");
        assert_eq!(date(11_016), "2000.02.29");
        assert_eq!(date(19_000), "2022.01.08");
        assert_eq!(date(20_737), "2026.10.11");
    }

    #[test]
    fn renders_game() {
        let record = |san: &str, clock_ms, eval| MoveRecord {
            uci: String::new(),
            san: san.into(),
            clock_ms,
            eval,
        };
        let state = MatchState {
            id: 3,
            date: "2026.10.11".into(),
            engines: [
                Participant {
                    id: "1".into(),
                    name: "Quote \"Bot\"".into(),
                },
                Participant {
                    id: "2".into(),
                    name: "Other".into(),
                },
            ],
            base_ms: 2500,
            increment_ms: 100,
            games: 2,
            max_plies: 400,
            status: Status::Finished,
            wins: [0, 1],
            draws: 0,
            finished: Vec::new(),
            current: None,
        };
        let game = GameRecord {
            number: 2,
            white: 1,
            moves: vec![
                record("e4", None, None),
                record(
                    "e5",
                    Some(2_499),
                    Some(Eval {
                        score: Score::Cp(-35),
                        depth: 12,
                    }),
                ),
                record("Qh5", Some(61_000), None),
                record(
                    "Nc6",
                    Some(3_723_456),
                    Some(Eval {
                        score: Score::Mate(-2),
                        depth: 30,
                    }),
                ),
            ],
            clocks: [0, 0],
            result: Some(GameResult::BlackWins),
            termination: Some(Termination::Timeout),
        };
        assert_eq!(
            super::game(&state, &game),
            "[Event \"Engine Arena match 3\"]\n[Site \"Engine Arena\"]\n[Date \"2026.10.11\"]\n[Round \"2\"]\n\
             [White \"Other\"]\n[Black \"Quote \\\"Bot\\\"\"]\n[Result \"0-1\"]\n[TimeControl \"2.5+0.1\"]\n\
             [Termination \"time forfeit\"]\n\n\
             1. e4 {book} e5 {[%eval -0.35,12] [%clk 0:00:02.4]} 2. Qh5 {[%clk 0:01:01.0]}\n\
             Nc6 {[%eval #-2,30] [%clk 1:02:03.4]} {Other loses on time} 0-1\n\n"
        );
    }
}
