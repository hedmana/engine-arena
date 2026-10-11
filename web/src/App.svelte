<script lang="ts">
  let status = $state('connecting')

  $effect(() => {
    const ws = new WebSocket(`ws://${location.host}/ws`)
    ws.onopen = () => ws.send('ping')
    ws.onmessage = (e) => (status = e.data)
    ws.onclose = () => (status = 'disconnected')
    return () => ws.close()
  })
</script>

<main>Engine Arena: server {status}</main>
