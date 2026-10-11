# Engine Arena

Local chess analysis board (Stockfish) and arena where your own UCI engines play each other. See [docs/PLAN.md](docs/PLAN.md).

## Dev

```sh
cd server && cargo run                   # http://127.0.0.1:7878
cd web && npm install && npm run dev     # http://localhost:5173, proxies /api and /ws
```

Requires `stockfish` on PATH (or set `STOCKFISH=/path/to/stockfish`) and [uv](https://docs.astral.sh/uv/) for Python engines. Uploaded engines live in `data/engines/`.

Release: `npm run build` in `web/`, then `cargo run --release` in `server/` and open http://127.0.0.1:7878.
