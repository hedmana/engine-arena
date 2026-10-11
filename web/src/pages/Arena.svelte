<script lang="ts">
  import type { Config } from 'chessground/config'
  import { cancelMatch, startMatch, type EngineEntry, type GameRecord } from '../lib/api'
  import type { ArenaFeed } from '../lib/arena.svelte'
  import Board from '../lib/Board.svelte'
  import { eloDifference } from '../lib/elo'
  import { formatScore } from '../lib/eval'

  let {
    feed,
    engines,
    active,
    onopen,
  }: { feed: ArenaFeed; engines: EngineEntry[]; active: boolean; onopen: (game: GameRecord) => void } = $props()

  let first = $state('')
  let second = $state('')
  let baseSeconds = $state(10)
  let incrementSeconds = $state(0.1)
  let games = $state(10)
  let error = $state('')
  let now = $state(performance.now())

  $effect(() => {
    if (!engines.some((e) => e.id === first)) first = engines[0]?.id ?? ''
    if (!engines.some((e) => e.id === second)) second = engines[1]?.id ?? first
  })

  const match = $derived(feed.match)
  const running = $derived(match?.status === 'running')
  const game = $derived(match?.current ?? null)
  const whiteToMove = $derived((game?.moves.length ?? 0) % 2 === 0)
  const lastEval = $derived(game?.moves.findLast((m) => m.eval)?.eval)
  const config: Config = $derived({ fen: feed.fen, lastMove: feed.lastMove, viewOnly: true, coordinates: false })
  const score = (i: 0 | 1) => (match ? match.wins[i] + match.draws / 2 : 0)
  const elo = $derived(match && eloDifference(match.wins[0], match.draws, match.wins[1]))
  const signed = (n: number) => `${n >= 0 ? '+' : ''}${n.toFixed(0)}`

  // Clocks tick locally between moves and are corrected by every move event.
  $effect(() => {
    if (!running || !active) return
    let frame = requestAnimationFrame(function tick(t) {
      now = t
      frame = requestAnimationFrame(tick)
    })
    return () => cancelAnimationFrame(frame)
  })

  function clock(white: boolean): string {
    const base = game?.clocks[white ? 0 : 1] ?? match?.baseMs ?? 0
    const ms = running && game && white === whiteToMove ? Math.max(0, base - (now - feed.turnStarted)) : base
    if (ms < 10_000) return (ms / 1000).toFixed(1)
    const seconds = Math.floor(ms / 1000)
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
  }

  const player = (g: GameRecord | null, white: boolean) => (g && match ? match.engines[white ? g.white : 1 - g.white].name : '')

  async function act(action: () => Promise<unknown>) {
    error = ''
    try {
      await action()
    } catch (e) {
      error = (e as Error).message
    }
  }

  const start = () =>
    act(() =>
      startMatch({
        engines: [first, second],
        baseMs: Math.round(baseSeconds * 1000),
        incrementMs: Math.round(incrementSeconds * 1000),
        games,
      }),
    )
</script>

<main>
  <section class="live">
    <div class="player">
      <span>{player(game, false) || 'Black'}</span>
      <span class="clock" class:thinking={running && game && !whiteToMove}>{clock(false)}</span>
    </div>
    <Board {config} />
    <div class="player">
      <span>{player(game, true) || 'White'}</span>
      <span class="clock" class:thinking={running && game && whiteToMove}>{clock(true)}</span>
    </div>
  </section>

  <aside>
    {#if match}
      <header>
        <div class="score">
          <span>{match.engines[0].name}</span>
          <strong>{score(0)} - {score(1)}</strong>
          <span>{match.engines[1].name}</span>
        </div>
        <table class="results">
          <thead><tr><th>Engine</th><th>Wins</th><th>Draws</th><th>Losses</th><th>Score</th></tr></thead>
          <tbody>
            {#each [0, 1] as const as i (i)}
              <tr>
                <td>{match.engines[i].name}</td>
                <td>{match.wins[i]}</td>
                <td>{match.draws}</td>
                <td>{match.wins[1 - i]}</td>
                <td>{score(i)} / {match.finished.length}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="muted">
          Elo difference: {elo ? `${signed(elo.elo)} ${elo.margin === undefined ? '(margin needs more games)' : `± ${elo.margin.toFixed(0)} (95%)`}` : 'n/a'};
          game {match.finished.length + (game ? 1 : 0)} of {match.games} ({match.status})
          {#if match.finished.length}<a href="/api/matches/{match.id}/pgn" download>Download PGN</a>{/if}
        </div>
        {#if running}<button onclick={() => act(cancelMatch)}>Cancel match</button>{/if}
      </header>
    {/if}

    {#if !running}
      <form onsubmit={(e) => (e.preventDefault(), start())}>
        <label>Engine 1 <select bind:value={first}>{#each engines as e (e.id)}<option value={e.id}>{e.name}</option>{/each}</select></label>
        <label>Engine 2 <select bind:value={second}>{#each engines as e (e.id)}<option value={e.id}>{e.name}</option>{/each}</select></label>
        <label>Base (s) <input type="number" min="0.1" step="0.1" bind:value={baseSeconds} /></label>
        <label>Increment (s) <input type="number" min="0" step="0.01" bind:value={incrementSeconds} /></label>
        <label>Games <input type="number" min="1" max="1000" bind:value={games} /></label>
        <button type="submit" disabled={!first || !second}>Start match</button>
      </form>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}

    {#if game}
      <h3>
        Game {game.number}
        {#if lastEval}<span class="muted">engine eval {formatScore({ ...lastEval, multiPv: 1, pvUci: [], pvSan: [] })} (depth {lastEval.depth})</span>{/if}
      </h3>
      <p class="moves">
        {#each game.moves as move, i (i)}
          {#if i % 2 === 0}<span class="muted">{i / 2 + 1}.</span>{/if}
          <span class:book={move.clockMs === undefined}>{move.san}</span>
        {/each}
      </p>
    {/if}

    {#if match?.finished.length}
      <h3>Games</h3>
      <div class="games">
        <table>
          <thead><tr><th>#</th><th>White</th><th>Black</th><th>Result</th><th>Ending</th><th>Plies</th></tr></thead>
          <tbody>
            {#each match.finished.toReversed() as g (g.number)}
              <tr onclick={() => onopen(g)} title="Open in analysis">
                <td>{g.number}</td>
                <td>{player(g, true)}</td>
                <td>{player(g, false)}</td>
                <td>{g.result}</td>
                <td>{g.termination?.replaceAll('_', ' ')}</td>
                <td>{g.moves.length}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </aside>
</main>

<style>
  main {
    display: grid;
    grid-template-columns: minmax(0, min(calc(100vh - 9rem), 640px)) minmax(320px, 520px);
    gap: 1rem;
    padding: 1rem;
    justify-content: center;
  }
  .player {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.35rem 0;
    font-weight: 600;
  }
  .clock {
    font: 600 1.2rem ui-monospace, monospace;
    padding: 0.1rem 0.5rem;
    border-radius: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
  }
  .clock.thinking {
    background: var(--accent);
    color: var(--on-accent);
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem;
    max-height: calc(100vh - 5rem);
    box-sizing: border-box;
    overflow: hidden;
  }
  .results {
    margin: 0.4rem 0;
  }
  .results td:not(:first-child),
  .results th:not(:first-child) {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .score {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    font-size: 1.1rem;
  }
  .score strong {
    font-size: 1.4rem;
    font-variant-numeric: tabular-nums;
  }
  form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.85rem;
    color: var(--muted);
  }
  h3 {
    margin: 0;
    font-size: 1rem;
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
  }
  .moves {
    margin: 0;
    max-height: 9rem;
    overflow-y: auto;
    line-height: 1.6;
    display: flex;
    flex-wrap: wrap;
    gap: 0 0.35rem;
  }
  .book {
    color: var(--muted);
    font-style: italic;
  }
  .games {
    overflow-y: auto;
    min-height: 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
  }
  th,
  td {
    text-align: left;
    padding: 0.25rem 0.4rem;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: var(--hover);
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
    font-weight: 400;
  }
  .error {
    color: var(--danger);
    margin: 0;
  }
  @media (max-width: 760px) {
    main {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
