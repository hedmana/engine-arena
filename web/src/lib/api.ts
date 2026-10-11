export interface EngineEntry {
  id: string
  name: string
  kind: 'python' | 'binary' | 'stockfish'
  uciName: string
  author: string
  options: Record<string, string>
}

export interface MoveRecord {
  uci: string
  san: string
  /** Mover's remaining time; absent for opening book moves. */
  clockMs?: number
  eval?: { cp?: number; mate?: number; depth: number }
}

export type Result = '1-0' | '0-1' | '1/2-1/2'

export interface GameRecord {
  number: number
  /** Index of the engine playing White. */
  white: 0 | 1
  moves: MoveRecord[]
  clocks: [number, number]
  result: Result | null
  termination: string | null
}

export interface MatchState {
  id: number
  date: string
  engines: [{ id: string; name: string }, { id: string; name: string }]
  baseMs: number
  incrementMs: number
  games: number
  maxPlies: number
  status: 'running' | 'finished' | 'cancelled'
  wins: [number, number]
  draws: number
  finished: GameRecord[]
  current: GameRecord | null
}

export interface MatchConfig {
  engines: [string, string]
  baseMs: number
  incrementMs: number
  games: number
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init)
  if (!response.ok) throw new Error((await response.json().catch(() => null))?.error ?? response.statusText)
  return response.status === 204 ? (undefined as T) : response.json()
}

const json = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
})

const form = (file: File, name?: string) => {
  const body = new FormData()
  body.set('file', file)
  if (name) body.set('name', name)
  return { method: 'POST', body }
}

const engine = (id: string) => `/api/engines/${encodeURIComponent(id)}`

export const listEngines = () => request<EngineEntry[]>('/api/engines')
export const uploadEngine = (file: File, name?: string) => request<EngineEntry>('/api/engines', form(file, name))
export const replaceEngine = (id: string, file: File) => request<EngineEntry>(engine(id), form(file))
export const updateEngine = (id: string, patch: { name?: string; options?: Record<string, string> }) =>
  request<EngineEntry>(engine(id), json('PATCH', patch))
export const deleteEngine = (id: string) => request<void>(engine(id), { method: 'DELETE' })
export const startMatch = (config: MatchConfig) => request<{ id: number }>('/api/matches', json('POST', config))
export const cancelMatch = () => request<void>('/api/matches/current', { method: 'DELETE' })
