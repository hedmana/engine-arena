<script lang="ts">
  import { Chessground } from 'chessground'
  import type { Api } from 'chessground/api'
  import type { Config } from 'chessground/config'
  import type { Key } from 'chessground/types'
  import 'chessground/assets/chessground.base.css'
  import 'chessground/assets/chessground.brown.css'
  import 'chessground/assets/chessground.cburnett.css'

  let { config, onmove }: { config: Config; onmove: (orig: Key, dest: Key) => void } = $props()

  let el: HTMLDivElement
  let api: Api | undefined

  $effect(() => {
    api = Chessground(el, {
      animation: { duration: 120 },
      premovable: { enabled: false },
      movable: { free: false, showDests: true, events: { after: (orig, dest) => onmove(orig, dest) } },
    })
    return () => api?.destroy()
  })

  // chessground diffs internally, so re-applying an unchanged position is cheap.
  $effect(() => api?.set(config))
</script>

<div class="board" bind:this={el}></div>

<style>
  .board {
    width: 100%;
    aspect-ratio: 1;
  }
</style>
