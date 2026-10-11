<script lang="ts">
  import { deleteEngine, listEngines, replaceEngine, updateEngine, uploadEngine, type EngineEntry } from '../lib/api'

  let { active, onchange }: { active: boolean; onchange: (engines: EngineEntry[]) => void } = $props()

  let engines = $state<EngineEntry[]>([])
  let name = $state('')
  let files = $state<FileList>()
  let picker: HTMLInputElement
  let busy = $state('')
  let error = $state('')
  /** Options edited as `Name=Value` lines, per engine id. */
  let drafts = $state<Record<string, string>>({})

  const optionText = (options: Record<string, string>) =>
    Object.entries(options)
      .map(([k, v]) => `${k}=${v}`)
      .join('\n')

  async function refresh() {
    engines = await listEngines()
    drafts = Object.fromEntries(engines.map((e) => [e.id, optionText(e.options)]))
    onchange(engines)
  }

  $effect(() => {
    if (active) run('', refresh)
  })

  async function run(label: string, action: () => Promise<unknown>) {
    busy = label
    error = ''
    try {
      await action()
      await refresh()
    } catch (e) {
      error = (e as Error).message
    } finally {
      busy = ''
    }
  }

  function parseOptions(text: string): Record<string, string> {
    return Object.fromEntries(
      text
        .split('\n')
        .filter((line) => line.includes('='))
        .map((line) => {
          const i = line.indexOf('=')
          return [line.slice(0, i).trim(), line.slice(i + 1).trim()]
        }),
    )
  }

  const upload = () =>
    files?.[0] &&
    run('Validating engine (a first Python run may install dependencies)...', async () => {
      await uploadEngine(files![0], name.trim() || undefined)
      name = ''
      picker.value = ''
      files = undefined
    })

  function pickReplacement(engine: EngineEntry) {
    const input = document.createElement('input')
    input.type = 'file'
    input.onchange = () => input.files?.[0] && run(`Validating ${engine.name}...`, () => replaceEngine(engine.id, input.files![0]))
    input.click()
  }
</script>

<section class="page">
  <h2>Upload an engine</h2>
  <p class="hint">A Python script (run with <code>uv run --script</code>) or any executable that speaks UCI. Start from <code>engines/template.py</code>.</p>
  <div class="row">
    <input type="file" bind:files bind:this={picker} />
    <input placeholder="Name (optional)" bind:value={name} />
    <button onclick={upload} disabled={!files?.length || !!busy}>Upload</button>
  </div>
  {#if busy}<p class="hint">{busy}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}

  <h2>Engines</h2>
  <table>
    <thead><tr><th>Name</th><th>Type</th><th>Reports as</th><th>UCI options</th><th></th></tr></thead>
    <tbody>
      {#each engines as engine (engine.id)}
        <tr>
          <td>
            <input
              value={engine.name}
              aria-label="Name"
              onchange={(e) => run('', () => updateEngine(engine.id, { name: e.currentTarget.value }))}
            />
          </td>
          <td>{engine.kind}</td>
          <td class="muted">{engine.uciName}{engine.author ? ` by ${engine.author}` : ''}</td>
          <td>
            <textarea rows="2" bind:value={drafts[engine.id]} placeholder="Name=Value" aria-label="UCI options"></textarea>
            {#if drafts[engine.id] !== optionText(engine.options)}
              <button onclick={() => run('', () => updateEngine(engine.id, { options: parseOptions(drafts[engine.id]) }))}>Save</button>
            {/if}
          </td>
          <td class="actions">
            {#if engine.kind !== 'stockfish'}
              <button onclick={() => pickReplacement(engine)} disabled={!!busy}>Replace</button>
              <button onclick={() => confirm(`Delete ${engine.name}?`) && run('', () => deleteEngine(engine.id))}>Delete</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

<style>
  .page {
    max-width: 1100px;
    margin: 0 auto;
    padding: 1rem;
  }
  h2 {
    font-size: 1.1rem;
    margin: 1rem 0 0.5rem;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    align-items: center;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  th,
  td {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
  textarea {
    width: 14rem;
    font: 12px ui-monospace, monospace;
  }
  .actions {
    white-space: nowrap;
  }
  .hint,
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
</style>
