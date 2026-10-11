<script lang="ts">
  import type { EngineEntry, GameRecord } from './lib/api'
  import { listEngines } from './lib/api'
  import { ArenaFeed } from './lib/arena.svelte'
  import { MoveTree } from './lib/tree'
  import Analysis from './pages/Analysis.svelte'
  import Arena from './pages/Arena.svelte'
  import Engines from './pages/Engines.svelte'

  const TABS = { analysis: 'Analysis', engines: 'Engines', arena: 'Arena' } as const
  type Tab = keyof typeof TABS

  const fromHash = (): Tab => (Object.keys(TABS) as Tab[]).find((t) => location.hash === `#/${t}`) ?? 'analysis'

  let tab = $state<Tab>(fromHash())
  let engines = $state<EngineEntry[]>([])
  let load = $state.raw<{ tree: MoveTree; headers: Map<string, string> }>()

  const feed = new ArenaFeed()
  $effect(() => () => feed.close())
  $effect(() => {
    listEngines().then((list) => (engines = list), () => {})
  })

  const running = $derived(feed.match?.status === 'running')

  function open(game: GameRecord) {
    const match = feed.match!
    const tree = new MoveTree()
    let node = tree.root
    for (const move of game.moves) node = tree.play(node, move.uci) ?? node
    const name = (i: number) => match.engines[i].name
    const headers = new Map([
      ['Event', `Engine Arena match ${match.id}`],
      ['Round', String(game.number)],
      ['White', name(game.white)],
      ['Black', name(1 - game.white)],
      ['Result', game.result ?? '*'],
    ])
    load = { tree, headers }
    location.hash = '#/analysis'
  }
</script>

<svelte:window onhashchange={() => (tab = fromHash())} />

<nav>
  {#each Object.entries(TABS) as [key, label] (key)}
    <a href="#/{key}" class:active={tab === key}>{label}</a>
  {/each}
  {#if running}<a class="live" href="#/arena">Match running</a>{/if}
</nav>

<div hidden={tab !== 'analysis'}><Analysis active={tab === 'analysis'} paused={running} {load} /></div>
{#if tab === 'engines'}<Engines active onchange={(list) => (engines = list)} />{/if}
<div hidden={tab !== 'arena'}><Arena {feed} {engines} active={tab === 'arena'} onopen={open} /></div>

<style>
  nav {
    display: flex;
    gap: 0.25rem;
    padding: 0.5rem 1rem 0;
    border-bottom: 1px solid var(--border);
  }
  a {
    padding: 0.4rem 0.8rem;
    color: var(--muted);
    text-decoration: none;
    border-bottom: 2px solid transparent;
  }
  a.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .live {
    margin-left: auto;
    color: var(--accent);
  }
</style>
