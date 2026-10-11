<script lang="ts">
  let status = $state('connecting')

  $effect(() => {
    const ws = new WebSocket(`ws://${location.host}/ws`)
    ws.onopen = () => (status = 'connected')
    ws.onclose = () => (status = 'disconnected')
    return () => ws.close()
  })
</script>

<main>Engine Arena: server {status}</main>
