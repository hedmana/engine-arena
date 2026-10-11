<script lang="ts">
  import type { Config } from 'chessground/config'
  import type { Key } from 'chessground/types'
  import { chessgroundDests } from 'chessops/compat'
  import { parseFen } from 'chessops/fen'
  import { parseSquare } from 'chessops/util'
  import Board from './lib/Board.svelte'
  import MoveList from './lib/MoveList.svelte'
  import { exportPgn, importPgn } from './lib/pgn'
  import { AnalysisSocket } from './lib/socket'
  import { MoveTree, positionOf, type TreeNode } from './lib/tree'

  const ROLES = ['queen', 'knight', 'rook', 'bishop'] as const
  const NAV_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'])

  // The tree is mutated in place; `version` re-renders the move list when it changes.
  const initial = new MoveTree()
  let tree = $state.raw(initial)
  let headers = $state.raw(new Map<string, string>())
  let current = $state.raw<TreeNode>(initial.root)
  let version = $state(0)
  let orientation = $state<'white' | 'black'>('white')
  let promotion = $state<{ orig: Key; dest: Key }>()
  let resets = $state(0)
  let text = $state('')
  let error = $state('')

  const socket = new AnalysisSocket(
    () => {},
    (message) => (error = message),
  )
  $effect(() => () => socket.close())

  const position = $derived(positionOf(current))
  const config: Config = $derived.by(() => {
    void resets
    return {
      fen: current.fen,
      orientation,
      turnColor: position.turn,
      check: position.isCheck(),
      lastMove: current.uci ? [current.uci.slice(0, 2) as Key, current.uci.slice(2, 4) as Key] : undefined,
      movable: { color: position.turn, dests: chessgroundDests(position) },
    }
  })

  $effect(() =>
    socket.analyze(
      tree.root.fen,
      MoveTree.path(current).map((n) => n.uci),
      3,
    ),
  )

  function play(uci: string) {
    const parent = current
    const count = parent.children.length
    const node = tree.play(parent, uci)
    if (!node) return void resets++
    if (parent.children.length !== count) version++
    current = node
  }

  function onmove(orig: Key, dest: Key) {
    const piece = position.board.get(parseSquare(orig)!)
    if (piece?.role === 'pawn' && (dest[1] === '1' || dest[1] === '8')) promotion = { orig, dest }
    else play(orig + dest)
  }

  function choosePromotion(role?: (typeof ROLES)[number]) {
    if (promotion && role) play(promotion.orig + promotion.dest + (role === 'knight' ? 'n' : role[0]))
    else resets++
    promotion = undefined
  }

  function target(key: string): TreeNode | undefined {
    const siblings = current.parent?.children ?? []
    const i = siblings.indexOf(current)
    switch (key) {
      case 'ArrowLeft':
        return current.parent
      case 'ArrowRight':
        return current.children[0]
      case 'ArrowUp':
        return siblings[i - 1]
      case 'ArrowDown':
        return siblings[i + 1]
      case 'Home':
        return tree.root
      case 'End':
        return MoveTree.lineEnd(current)
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
    if (promotion) {
      if (e.key === 'Escape') choosePromotion()
      return
    }
    if (e.key === 'f') flip()
    if (!NAV_KEYS.has(e.key)) return
    e.preventDefault()
    current = target(e.key) ?? current
  }

  const flip = () => (orientation = orientation === 'white' ? 'black' : 'white')

  function load() {
    const input = text.trim()
    if (!input) return
    try {
      if (parseFen(input).isOk) {
        tree = new MoveTree(input)
        headers = new Map()
      } else {
        const game = importPgn(input)
        tree = game.tree
        headers = game.headers
      }
      current = tree.root
      version++
      error = ''
    } catch (e) {
      error = (e as Error).message
    }
  }

  function promoteLine() {
    MoveTree.promote(current)
    version++
  }

  function deleteFromHere() {
    current = MoveTree.delete(current)
    version++
  }

  const promotionStyle = $derived.by(() => {
    if (!promotion) return ''
    const file = promotion.dest.charCodeAt(0) - 97
    const atTop = (promotion.dest[1] === '8') === (orientation === 'white')
    const left = (orientation === 'white' ? file : 7 - file) * 12.5
    return `left: ${left}%; ${atTop ? 'top: 0' : 'bottom: 0; flex-direction: column-reverse'}`
  })
</script>

<svelte:window {onkeydown} />

<main>
  <section class="board">
    <Board {config} {onmove} />
    {#if promotion}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="promotion cg-wrap" onclick={() => choosePromotion()}>
        <div class="choices" style={promotionStyle}>
          {#each ROLES as role (role)}
            <button aria-label={role} onclick={(e) => (e.stopPropagation(), choosePromotion(role))}>
              <piece class="{role} {position.turn}"></piece>
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </section>

  <aside>
    <div class="toolbar">
      <button onclick={() => (current = tree.root)} title="Start (Home)">|&lt;</button>
      <button onclick={() => (current = current.parent ?? current)} title="Back (Left)">&lt;</button>
      <button onclick={() => (current = current.children[0] ?? current)} title="Forward (Right)">&gt;</button>
      <button onclick={() => (current = MoveTree.lineEnd(current))} title="End (End)">&gt;|</button>
      <button onclick={flip} title="Flip board (f)">Flip</button>
      <button onclick={promoteLine} disabled={!current.parent} title="Promote variation">Promote</button>
      <button onclick={deleteFromHere} disabled={!current.parent} title="Delete from here">Delete</button>
    </div>

    {#key version}
      <MoveList root={tree.root} {current} onselect={(node) => (current = node)} />
    {/key}

    <input class="fen" readonly value={current.fen} onfocus={(e) => e.currentTarget.select()} aria-label="FEN" />
    <textarea bind:value={text} rows="5" placeholder="Paste PGN or FEN"></textarea>
    <div class="toolbar">
      <button onclick={load}>Load</button>
      <button onclick={() => (text = exportPgn(tree, headers))}>Export PGN</button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
  </aside>
</main>

<style>
  main {
    display: grid;
    grid-template-columns: minmax(0, min(calc(100vh - 2rem), 720px)) minmax(280px, 420px);
    gap: 1rem;
    padding: 1rem;
    justify-content: center;
  }
  .board {
    position: relative;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-height: calc(100vh - 2rem);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem;
    box-sizing: border-box;
  }
  aside :global(.moves) {
    flex: 1;
    min-height: 8rem;
  }
  .toolbar {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
  .toolbar button {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border);
    background: var(--bg);
    color: inherit;
    border-radius: 4px;
    cursor: pointer;
  }
  .toolbar button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .fen,
  textarea {
    font: 12px ui-monospace, monospace;
    background: var(--bg);
    color: inherit;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem;
  }
  .error {
    color: var(--danger);
    margin: 0;
  }
  .promotion {
    position: absolute;
    inset: 0;
    z-index: 10;
    background: rgb(0 0 0 / 0.4);
  }
  .choices {
    position: absolute;
    width: 12.5%;
    height: 50%;
    display: flex;
    flex-direction: column;
  }
  .choices button {
    flex: 1;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: #b0b0b0;
    cursor: pointer;
  }
  .choices button:hover {
    background: #e0e0e0;
  }
  .promotion.cg-wrap piece {
    position: static;
    display: block;
    width: 100%;
    height: 100%;
  }
  @media (max-width: 760px) {
    main {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
