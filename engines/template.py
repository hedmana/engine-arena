# /// script
# requires-python = ">=3.10"
# dependencies = ["chess>=1.10"]
# ///
"""Engine Arena UCI template: write select_move, the UCI plumbing below is done for you."""

import itertools
import sys
from dataclasses import dataclass

import chess

NAME = "My Engine"
AUTHOR = "Me"


@dataclass
class Clock:
    """Search limits from `go`, in seconds. Times are for the side to move; None if not given."""

    time_left: float | None = None
    increment: float = 0.0
    moves_to_go: int | None = None
    movetime: float | None = None
    depth: int | None = None

    def budget(self) -> float:
        """A simple time allocation for this move."""
        if self.movetime is not None:
            return self.movetime
        if self.time_left is None:
            return 1.0
        return max(
            0.01, self.time_left / (self.moves_to_go or 30) + self.increment / 2 - 0.05
        )


def select_move(board: chess.Board, clock: Clock) -> chess.Move:
    """Return a legal move for the side to move. Replace this with your engine."""
    return next(iter(board.legal_moves))


# UCI plumbing below this line.


def main() -> None:
    board = chess.Board()
    for line in sys.stdin:
        tokens = line.split()
        if not tokens:
            continue
        command, args = tokens[0], tokens[1:]
        if command == "uci":
            send(f"id name {NAME}", f"id author {AUTHOR}", "uciok")
        elif command == "isready":
            send("readyok")
        elif command == "ucinewgame":
            board = chess.Board()
        elif command == "position":
            board = parse_position(args)
        elif command == "go":
            move = (
                select_move(board.copy(), parse_go(args, board.turn))
                if any(board.legal_moves)
                else None
            )
            send(f"bestmove {move.uci() if move else '0000'}")
        elif command == "quit":
            break


def send(*lines: str) -> None:
    print("\n".join(lines), flush=True)


def parse_position(args: list[str]) -> chess.Board:
    moves = args.index("moves") if "moves" in args else len(args)
    board = (
        chess.Board() if args[0] == "startpos" else chess.Board(" ".join(args[1:moves]))
    )
    for uci in args[moves + 1 :]:
        board.push_uci(uci)
    return board


def parse_go(args: list[str], turn: chess.Color) -> Clock:
    values = {
        key: int(value)
        for key, value in itertools.pairwise(args)
        if value.lstrip("-").isdigit()
    }
    side = "w" if turn == chess.WHITE else "b"
    seconds = lambda key: values[key] / 1000 if key in values else None
    return Clock(
        time_left=seconds(f"{side}time"),
        increment=seconds(f"{side}inc") or 0.0,
        moves_to_go=values.get("movestogo"),
        movetime=seconds("movetime"),
        depth=values.get("depth"),
    )


if __name__ == "__main__":
    main()
