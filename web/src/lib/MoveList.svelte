<script lang="ts">
  import type { TreeNode } from './tree'

  let { root, current, onselect }: { root: TreeNode; current: TreeNode; onselect: (node: TreeNode) => void } = $props()

  const NAGS: Record<number, string> = { 1: '!', 2: '?', 3: '!!', 4: '??', 5: '!?', 6: '?!' }

  let list: HTMLElement
  $effect(() => list.querySelector(`[data-id="${current.id}"]`)?.scrollIntoView({ block: 'nearest' }))

  /** `start` followed by its main continuation. */
  function line(start: TreeNode): TreeNode[] {
    const nodes = [start]
    for (let n = start; n.children.length; n = n.children[0]) nodes.push(n.children[0])
    return nodes
  }

  const hasVariations = (node: TreeNode) => node.parent!.children[0] === node && node.parent!.children.length > 1
  const numbered = (nodes: TreeNode[], i: number) =>
    nodes[i].white || i === 0 || hasVariations(nodes[i - 1]) || !!nodes[i - 1].comments?.length
</script>

{#snippet variation(start: TreeNode)}
  {@const nodes = line(start)}
  {#each nodes as node, i (node.id)}
    {#each node.startingComments ?? [] as comment}<span class="comment">{comment}</span>{/each}
    {#if numbered(nodes, i)}<span class="number">{node.moveNumber}{node.white ? '.' : '...'}</span>{/if}
    <button data-id={node.id} class:active={node === current} onclick={() => onselect(node)}>
      {node.san}{node.nags?.map((n) => NAGS[n] ?? '').join('')}
    </button>
    {#each node.comments ?? [] as comment}<span class="comment">{comment}</span>{/each}
    {#if hasVariations(node)}
      {#each node.parent!.children.slice(1) as alternative (alternative.id)}
        <span class="variation">({@render variation(alternative)})</span>
      {/each}
    {/if}
  {/each}
{/snippet}

<div class="moves" bind:this={list}>
  {#each root.comments ?? [] as comment}<span class="comment">{comment}</span>{/each}
  {#if root.children.length}{@render variation(root.children[0])}{/if}
</div>

<style>
  .moves {
    overflow-y: auto;
    line-height: 1.8;
    font-variant-numeric: tabular-nums;
  }
  button {
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-weight: 600;
    padding: 0 0.2em;
    border-radius: 3px;
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  button.active {
    background: var(--accent);
    color: var(--on-accent);
  }
  .number {
    color: var(--muted);
  }
  .variation {
    color: var(--muted);
  }
  .variation button {
    font-weight: 400;
  }
  .comment {
    color: var(--comment);
    font-style: italic;
  }
</style>
