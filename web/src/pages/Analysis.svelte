<script lang="ts">
  import { untrack } from 'svelte'
  import type { Config } from 'chessground/config'
  import type { DrawShape } from 'chessground/draw'
  import type { Key } from 'chessground/types'
  import { chessgroundDests } from 'chessops/compat'
  import { parseFen } from 'chessops/fen'
  import { parseSquare } from 'chessops/util'
  import Board from '../lib/Board.svelte'
  import { Engine } from '../lib/engine.svelte'
  import EnginePanel from '../lib/EnginePanel.svelte'
  import { whitePercent } from '../lib/eval'
  import EvalBar from '../lib/EvalBar.svelte'
  import MoveList from '../lib/MoveList.svelte'
  import { exportPgn, importPgn } from '../lib/pgn'
  import { MoveTree, positionOf, type TreeNode } from '../lib/tree'

  const ROLES = ['queen', 'knight', 'rook', 'bishop'] as const
  const NAV_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'])

  let {
    active,
    paused,
    load,
  }: { active: boolean; paused: boolean; load?: { tree: MoveTree; headers: Map<string, string> } } = $props()

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

  const engine = new Engine()
  $effect(() => () => engine.close())

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

  $effect(() => {
    if (!load) return
    const game = load
    untrack(() => {
      tree = game.tree
      headers = game.headers
      current = game.tree.root
      version++
    })
  })

  $effect(() => {
    if (active) engine.analyze(tree.root.fen, MoveTree.path(current).map((n) => n.uci), current.fen)
    else engine.suspend()
  })

  const terminal = $derived(
    position.isCheckmate() ? 'Checkmate' : position.isStalemate() ? 'Stalemate' : position.isInsufficientMaterial() ? 'Draw' : undefined,
  )
  const white = $derived.by(() => {
    if (engine.enabled && engine.analysis) return whitePercent(engine.analysis.lines[0])
    if (terminal === 'Checkmate') return position.turn === 'white' ? 0 : 100
    return 50
  })

  // A string compares by value, so the arrows only re-render when the best moves change.
  const bestMoves = $derived(engine.enabled ? (engine.analysis?.lines.map((l) => l.pvUci[0]).join(' ') ?? '') : '')
  const shapes: DrawShape[] = $derived(
    bestMoves
      ? bestMoves.split(' ').map((uci, i) => ({
          orig: uci.slice(0, 2) as Key,
          dest: uci.slice(2, 4) as Key,
          brush: i === 0 ? 'paleBlue' : 'paleGrey',
          modifiers: { lineWidth: i === 0 ? 14 : 8 },
        }))
      : [],
  )

  /** Plays `moves` from the current node, reusing existing branches. */
  function play(...moves: string[]) {
    let node = current
    let added = false
    for (const uci of moves) {
      const count = node.children.length
      const next = tree.play(node, uci)
      if (!next) break
      added ||= node.children.length !== count
      node = next
    }
    if (node === current) resets++
    if (added) version++
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
    if (!active) return
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

  function importText() {
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
  <EvalBar {white} {orientation} />
  <section class="board">
    <Board {config} {shapes} {onmove} />
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
    {#if paused}<p class="paused">Analysis is paused while a match runs.</p>{/if}
    <EnginePanel {engine} whiteToMove={position.turn === 'white'} fullmoves={position.fullmoves} {terminal} onplay={(moves) => play(...moves)} />
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
      <button onclick={importText}>Load</button>
      <button onclick={() => (text = exportPgn(tree, headers))}>Export PGN</button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
  </aside>
</main>

<style>
  main {
    display: grid;
    grid-template-columns: 14px minmax(0, min(calc(100vh - 5rem), 720px)) minmax(300px, 440px);
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
    max-height: calc(100vh - 5rem);
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
  .paused {
    margin: 0;
    color: var(--muted);
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
      grid-template-columns: 14px minmax(0, 1fr);
    }
    aside {
      grid-column: 1 / -1;
    }
  }
</style>
