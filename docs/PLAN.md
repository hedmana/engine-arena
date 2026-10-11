# Engine Arena: implementation plan

Local chess app: (1) a fast Stockfish analysis board, (2) an arena where your own UCI engines play each other.

## Architecture

```
browser (Svelte 5 + chessground + chessops)
   |  WebSocket, JSON, <= 20 msg/s
server (Rust, axum + tokio, 127.0.0.1 only)
   |-- stockfish (native, one persistent process)
   '-- user engines (subprocesses, UCI)
```

- `server/` Rust backend; serves `web/dist` in release
- `web/` Svelte frontend
- `engines/` Python template (`select_move(board, clock)`, UCI handled for you) and examples
- `data/` (gitignored) uploaded engines, match PGNs

## Performance rules

1. Moves and navigation never wait on the server; chessops computes legal moves in the browser.
2. One persistent Stockfish; hash stays warm across positions (no `ucinewgame` within a game).
3. Engine stdout is drained continuously so an engine never blocks on a full pipe.
4. Latest-wins: rapid navigation analyzes only the newest position; generation ids drop stale output.
5. Server coalesces info lines to at most 20 updates/s; browser applies them once per animation frame.
6. Eval bar and arrows update without re-rendering the board.
7. A running match pauses analysis so engines get the cores.

Budgets: move/navigation < 16 ms, first eval on a new position < 100 ms, NPS within 2% of standalone Stockfish.

## Security

- Uploaded engines are arbitrary code. Server binds 127.0.0.1 only and rejects requests whose `Origin`/`Host` is not its own.
- Server-generated upload paths, size cap, stdout line cap, RSS watchdog, process-group kill.
- Not safe to host for other users without real sandboxing.

## Milestones

| | Issues |
|---|---|
| M0 Setup | #1 scaffold + CI |
| M1 Analysis board | #2 UCI driver, #3 analysis WS API, #4 board + move tree, #5 engine panel |
| M2 Arena | #6 engine registry, #7 Python template, #8 match runner, #9 match UI, #10 results + PGN |

Order: #1, #2, #3, then #4 and #5. M2 reuses the #2 driver: #6, #7, #8, #9, #10.

## Dependencies

- Rust: tokio, axum, tower-http, serde, serde_json, shakmaty, tracing, tracing-subscriber, libc
- Web: svelte, vite, @sveltejs/vite-plugin-svelte, typescript, svelte-check, chessground, chessops, vitest
- Engine template: python-chess (PEP 723 inline, run via `uv`)

## License

Stockfish, chessground, chessops, and shakmaty are GPL-3.0, so this repo is GPL-3.0.

## Later

Concurrent games, game review and eval graph, SPRT, multi-file engine uploads, real sandboxing.
