<script lang="ts">
  import type { Engine } from './engine.svelte'
  import { formatScore } from './eval'

  let {
    engine,
    whiteToMove,
    fullmoves,
    terminal,
    onplay,
  }: {
    engine: Engine
    whiteToMove: boolean
    fullmoves: number
    terminal?: string
    onplay: (moves: string[]) => void
  } = $props()

  const PV_PLIES = 16
  const HASH_MB = [256, 512, 1024, 2048, 4096, 8192]
  const cores = navigator.hardwareConcurrency || 4

  const speed = (nps: number) => (nps >= 1e6 ? `${(nps / 1e6).toFixed(1)}M` : `${Math.round(nps / 1e3)}k`)

  /** Move number label before ply `i` of a PV, if one is due. */
  function number(i: number): string {
    const white = whiteToMove === (i % 2 === 0)
    const n = fullmoves + Math.floor((i + (whiteToMove ? 0 : 1)) / 2)
    return white ? `${n}.` : i === 0 ? `${n}...` : ''
  }
</script>

<section class="engine">
  <header>
    <label><input type="checkbox" bind:checked={engine.enabled} /> Stockfish</label>
    {#if engine.enabled && engine.analysis}
      <strong>{formatScore(engine.analysis.lines[0])}</strong>
      <span class="stats">depth {engine.analysis.depth}, {speed(engine.analysis.nps)} nodes/s</span>
    {:else if terminal}
      <strong>{terminal}</strong>
    {/if}
    <select bind:value={engine.multiPv} aria-label="Lines">
      {#each [1, 2, 3, 4, 5] as n (n)}<option value={n}>{n} {n === 1 ? 'line' : 'lines'}</option>{/each}
    </select>
  </header>

  {#if engine.enabled && engine.analysis}
    <ol>
      {#each engine.analysis.lines as line (line.multiPv)}
        <li>
          <span class="score">{formatScore(line)}</span>
          <span class="pv">
            {#each line.pvSan.slice(0, PV_PLIES) as san, i (i)}
              <span class="number">{number(i)}</span><button onclick={() => onplay(line.pvUci.slice(0, i + 1))}>{san}</button>
            {/each}
          </span>
        </li>
      {/each}
    </ol>
  {/if}

  {#if engine.error}<p class="error">{engine.error}</p>{/if}

  <details>
    <summary>Engine settings</summary>
    <label>
      Threads
      <input type="number" min="1" max={cores} bind:value={engine.threads} onchange={() => engine.configure()} />
    </label>
    <label>
      Hash
      <select bind:value={engine.hashMb} onchange={() => engine.configure()}>
        {#each HASH_MB as mb (mb)}<option value={mb}>{mb} MB</option>{/each}
      </select>
    </label>
  </details>
</section>

<style>
  .engine {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.5rem;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
  }
  header strong {
    font-size: 1.3rem;
    font-variant-numeric: tabular-nums;
  }
  .stats {
    color: var(--muted);
    font-size: 0.85rem;
  }
  header select {
    margin-left: auto;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 0.9rem;
  }
  li {
    display: flex;
    gap: 0.5rem;
    white-space: nowrap;
    overflow: hidden;
    line-height: 1.7;
  }
  .score {
    flex: none;
    width: 3.6em;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .pv {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .number {
    color: var(--muted);
  }
  .pv button {
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    padding: 0 0.15em;
    margin-right: 0.15em;
    border-radius: 3px;
    cursor: pointer;
  }
  .pv button:hover {
    background: var(--hover);
  }
  details label {
    display: inline-flex;
    gap: 0.4rem;
    align-items: center;
    margin: 0.4rem 1rem 0 0;
    font-size: 0.9rem;
  }
  details input {
    width: 4em;
  }
  .error {
    color: var(--danger);
    margin: 0;
  }
</style>
