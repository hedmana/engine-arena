# Engine Arena

Local chess analysis board (Stockfish) and arena where your own UCI engines play each other. See [docs/PLAN.md](docs/PLAN.md).

## Dev

```sh
cd server && cargo run                   # http://127.0.0.1:7878
cd web && npm install && npm run dev     # http://localhost:5173, proxies /api and /ws
```

Requires `stockfish` on PATH (or set `STOCKFISH=/path/to/stockfish`) and [uv](https://docs.astral.sh/uv/) for Python engines. Uploaded engines live in `data/engines/`.

Release: `npm run build` in `web/`, then `cargo run --release` in `server/` and open http://127.0.0.1:7878.

## Writing an engine

Copy `engines/template.py`, implement `select_move(board, clock)`, and upload it:

```sh
curl -F file=@my_engine.py http://127.0.0.1:7878/api/engines
```

- Python files run with `uv run --script`; declare dependencies in the PEP 723 header.
- `board` is a [python-chess](https://python-chess.readthedocs.io/) `Board` copy; `clock.budget()` suggests how many seconds to spend.
- Any executable that speaks UCI over stdin/stdout works too.
- Running out of time, illegal moves, and crashes lose the game.
- Examples: `engines/examples/random_mover.py`, `engines/examples/greedy_material.py`.
