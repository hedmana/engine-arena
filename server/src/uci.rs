pub mod info;

use std::{
    fmt::{self, Display, Write as _},
    io,
    path::Path,
    process::Stdio,
    time::Duration,
};

use shakmaty::{CastlingMode, Chess, EnPassantMode, Position, fen::Fen, uci::UciMove};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    time::timeout,
};

const MAX_LINE: usize = 64 * 1024;

/// A running UCI engine. Dropping it kills the engine's whole process group.
pub struct Engine {
    pub name: String,
    pub author: String,
    stdin: ChildStdin,
    lines: mpsc::UnboundedReceiver<String>,
    pgid: libc::pid_t,
    _child: Child,
}

impl Engine {
    /// Spawns the engine in its own process group and completes the `uci`/`uciok` handshake.
    pub async fn start(
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        limit: Duration,
    ) -> io::Result<Self> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn()?;
        let pgid = child.id().expect("unreaped child has a pid") as libc::pid_t;
        let (tx, lines) = mpsc::unbounded_channel();
        tokio::spawn(drain(child.stdout.take().expect("stdout is piped"), tx));
        let mut engine = Self {
            name: String::new(),
            author: String::new(),
            stdin: child.stdin.take().expect("stdin is piped"),
            lines,
            pgid,
            _child: child,
        };
        engine.send("uci").await?;
        timeout(limit, async {
            loop {
                let line = engine.recv().await.ok_or_else(exited)?;
                if line == "uciok" {
                    return Ok::<_, io::Error>(());
                }
                if let Some(name) = line.strip_prefix("id name ") {
                    engine.name = name.to_owned();
                } else if let Some(author) = line.strip_prefix("id author ") {
                    engine.author = author.to_owned();
                }
            }
        })
        .await
        .map_err(|_| timed_out("uciok"))??;
        Ok(engine)
    }

    pub async fn set_option(&mut self, name: &str, value: impl Display) -> io::Result<()> {
        let command = format!("setoption name {name} value {value}");
        if command.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "newline in UCI option",
            ));
        }
        self.send(&command).await
    }

    /// Sends `isready` and waits for `readyok`, discarding output in between.
    pub async fn ready(&mut self, limit: Duration) -> io::Result<()> {
        self.send("isready").await?;
        timeout(limit, async {
            while let Some(line) = self.recv().await {
                if line == "readyok" {
                    return Ok(());
                }
            }
            Err(exited())
        })
        .await
        .map_err(|_| timed_out("readyok"))?
    }

    pub(crate) async fn send(&mut self, command: &str) -> io::Result<()> {
        self.stdin
            .write_all(format!("{command}\n").as_bytes())
            .await
    }

    /// Next output line; `None` once the engine has exited.
    pub(crate) async fn recv(&mut self) -> Option<String> {
        self.lines.recv().await
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: killpg has no memory-safety preconditions. The child is reaped only
        // after this runs, so its pid (and pgid) cannot have been reused.
        unsafe { libc::killpg(self.pgid, libc::SIGKILL) };
    }
}

/// Reads stdout continuously so the engine never blocks on a full pipe.
async fn drain(stdout: ChildStdout, lines: mpsc::UnboundedSender<String>) {
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut reader)
            .take(MAX_LINE as u64)
            .read_until(b'\n', &mut buf)
            .await
        {
            Ok(0) | Err(_) => return,
            Ok(n) if n == MAX_LINE && buf.last() != Some(&b'\n') => {
                tracing::warn!("engine output line exceeds {MAX_LINE} bytes, disconnecting");
                return;
            }
            Ok(_) => {}
        }
        let line = buf.trim_ascii_end();
        if line.starts_with(b"info ") && line.windows(10).any(|w| w == b" currmove ") {
            continue;
        }
        if lines
            .send(String::from_utf8_lossy(line).into_owned())
            .is_err()
        {
            return;
        }
    }
}

fn exited() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "engine exited")
}

fn timed_out(waiting_for: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("timed out waiting for {waiting_for}"),
    )
}

/// A validated root position plus legal moves, rendered as a UCI `position` command.
/// Sending root + moves (not the final FEN) gives the engine the history it needs for repetitions.
#[derive(Debug, Clone)]
pub struct GamePosition {
    command: String,
    position: Chess,
}

impl GamePosition {
    pub fn new(root_fen: &str, moves: &[impl AsRef<str>]) -> Result<Self, InvalidPosition> {
        let mut position: Chess = root_fen
            .parse::<Fen>()
            .map_err(invalid)?
            .into_position(CastlingMode::Standard)
            .map_err(invalid)?;
        let mut command = format!(
            "position fen {}",
            Fen::from_position(&position, EnPassantMode::Legal)
        );
        if !moves.is_empty() {
            command.push_str(" moves");
        }
        for uci in moves {
            let m = uci
                .as_ref()
                .parse::<UciMove>()
                .map_err(invalid)?
                .to_move(&position)
                .map_err(invalid)?;
            write!(command, " {}", m.to_uci(CastlingMode::Standard)).expect("writing to a String");
            position.play_unchecked(m);
        }
        Ok(Self { command, position })
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn position(&self) -> &Chess {
        &self.position
    }
}

#[derive(Debug)]
pub struct InvalidPosition(String);

impl Display for InvalidPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid position: {}", self.0)
    }
}

impl std::error::Error for InvalidPosition {}

fn invalid(e: impl Display) -> InvalidPosition {
    InvalidPosition(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn position_command() {
        let p = GamePosition::new(START, &["e2e4", "e7e5", "g1f3"]).unwrap();
        assert_eq!(
            p.command(),
            format!("position fen {START} moves e2e4 e7e5 g1f3")
        );
        assert_eq!(p.position().turn(), shakmaty::Color::Black);
        assert_eq!(
            GamePosition::new(START, &[] as &[&str]).unwrap().command(),
            format!("position fen {START}")
        );
    }

    #[test]
    fn castling_is_normalized() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert!(
            GamePosition::new(fen, &["e1h1"])
                .unwrap()
                .command()
                .ends_with("moves e1g1")
        );
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(GamePosition::new("8/8/8/8/8/8/8/8 w - - 0 1", &[] as &[&str]).is_err());
        assert!(GamePosition::new(START, &["e2e5"]).is_err());
        assert!(GamePosition::new(START, &["e2e4\ngo infinite"]).is_err());
        assert!(GamePosition::new(&format!("{START}\nquit"), &[] as &[&str]).is_err());
    }
}
