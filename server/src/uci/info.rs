use std::{ops::Neg, str::FromStr, str::SplitAsciiWhitespace};

use shakmaty::{Color, uci::UciMove};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Neg for Score {
    type Output = Self;

    fn neg(self) -> Self {
        match self {
            Self::Cp(cp) => Self::Cp(-cp),
            Self::Mate(n) => Self::Mate(-n),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Bound {
    #[default]
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    pub multipv: Option<u32>,
    pub score: Option<Score>,
    pub bound: Bound,
    pub wdl: Option<[u32; 3]>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub hashfull: Option<u32>,
    pub pv: Vec<UciMove>,
}

impl Info {
    /// UCI scores are from the side to move; this flips them to White's point of view.
    pub fn to_white_pov(mut self, turn: Color) -> Self {
        if turn == Color::Black {
            self.score = self.score.map(Neg::neg);
            self.wdl = self.wdl.map(|[w, d, l]| [l, d, w]);
            self.bound = match self.bound {
                Bound::Lower => Bound::Upper,
                Bound::Upper => Bound::Lower,
                Bound::Exact => Bound::Exact,
            };
        }
        self
    }
}

/// Parses a UCI `info` line. Returns `None` for other lines, malformed fields,
/// and `string`/`currmove`/`refutation`/`currline` lines, without allocating.
pub fn parse(line: &str) -> Option<Info> {
    let mut tokens = line.split_ascii_whitespace();
    if tokens.next()? != "info" {
        return None;
    }
    let mut info = Info::default();
    while let Some(token) = tokens.next() {
        match token {
            "depth" => info.depth = Some(num(&mut tokens)?),
            "seldepth" => info.seldepth = Some(num(&mut tokens)?),
            "multipv" => info.multipv = Some(num(&mut tokens)?),
            "nodes" => info.nodes = Some(num(&mut tokens)?),
            "nps" => info.nps = Some(num(&mut tokens)?),
            "hashfull" => info.hashfull = Some(num(&mut tokens)?),
            "score" => {
                let kind = tokens.next()?;
                let value = num(&mut tokens)?;
                info.score = Some(match kind {
                    "cp" => Score::Cp(value),
                    "mate" => Score::Mate(value),
                    _ => return None,
                });
            }
            "lowerbound" => info.bound = Bound::Lower,
            "upperbound" => info.bound = Bound::Upper,
            "wdl" => {
                info.wdl = Some([num(&mut tokens)?, num(&mut tokens)?, num(&mut tokens)?]);
            }
            "pv" => {
                info.pv = tokens.map_while(|t| t.parse().ok()).collect();
                break;
            }
            "string" | "currmove" | "refutation" | "currline" => return None,
            _ => {}
        }
    }
    Some(info)
}

fn num<T: FromStr>(tokens: &mut SplitAsciiWhitespace) -> Option<T> {
    tokens.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pv(moves: &[&str]) -> Vec<UciMove> {
        moves.iter().map(|m| m.parse().unwrap()).collect()
    }

    #[test]
    fn full_line() {
        let info = parse("info depth 14 seldepth 21 multipv 2 score cp -30 wdl 6 935 59 nodes 507884 nps 1175657 hashfull 155 tbhits 0 time 432 pv a7a6 b5a4 g8f6").unwrap();
        assert_eq!(
            info,
            Info {
                depth: Some(14),
                seldepth: Some(21),
                multipv: Some(2),
                score: Some(Score::Cp(-30)),
                bound: Bound::Exact,
                wdl: Some([6, 935, 59]),
                nodes: Some(507884),
                nps: Some(1175657),
                hashfull: Some(155),
                pv: pv(&["a7a6", "b5a4", "g8f6"]),
            }
        );
    }

    #[test]
    fn bounds_and_mates() {
        let info = parse("info depth 21 seldepth 37 multipv 1 score cp -19 upperbound wdl 9 953 38 nodes 3145586 pv g8f6 e1g1").unwrap();
        assert_eq!(
            (info.score, info.bound),
            (Some(Score::Cp(-19)), Bound::Upper)
        );
        let info = parse("info depth 30 score cp 55 lowerbound nodes 1 pv e2e4").unwrap();
        assert_eq!(info.bound, Bound::Lower);
        let info =
            parse("info depth 10 seldepth 2 multipv 1 score mate 1 wdl 1000 0 0 nodes 200 pv d1d8")
                .unwrap();
        assert_eq!((info.score, info.pv), (Some(Score::Mate(1)), pv(&["d1d8"])));
        let info = parse("info depth 0 score mate 0").unwrap();
        assert_eq!((info.score, info.pv.len()), (Some(Score::Mate(0)), 0));
    }

    #[test]
    fn white_pov() {
        let info = parse("info depth 5 score mate -3 lowerbound wdl 0 10 990 pv e7e5").unwrap();
        let white = info.clone().to_white_pov(Color::White);
        assert_eq!(white, info);
        let black = info.to_white_pov(Color::Black);
        assert_eq!(
            (black.score, black.bound, black.wdl),
            (Some(Score::Mate(3)), Bound::Upper, Some([990, 10, 0]))
        );
    }

    #[test]
    fn skipped_lines() {
        for line in [
            "info depth 30 currmove e2e4 currmovenumber 1",
            "info string NNUE evaluation using nn-1a298aa575a0.nnue",
            "info refutation d1h5 g6h5",
            "bestmove e2e4 ponder e7e5",
            "uciok",
            "",
            "info depth x",
            "info score wdl 1 2 3",
        ] {
            assert_eq!(parse(line), None, "{line}");
        }
    }

    #[test]
    fn pv_stops_at_garbage() {
        assert_eq!(parse("info pv e2e4 zz e7e5").unwrap().pv, pv(&["e2e4"]));
    }
}
