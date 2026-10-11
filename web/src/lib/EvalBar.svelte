<script lang="ts">
  let { white, orientation }: { white: number; orientation: 'white' | 'black' } = $props()
</script>

<div class="bar" class:flipped={orientation === 'black'} title="{white.toFixed(0)}% White">
  <div class="fill" style:transform="scaleY({white / 100})"></div>
  <div class="mid"></div>
</div>

<style>
  .bar {
    position: relative;
    width: 14px;
    background: #403d39;
    border-radius: 3px;
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0;
    background: #f0efe9;
    transform-origin: bottom;
    transition: transform 0.4s ease;
    will-change: transform;
  }
  .flipped .fill {
    transform-origin: top;
  }
  .mid {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    background: #d64f00;
    opacity: 0.6;
  }
</style>
