export interface AnalysisLine {
  multiPv: number
  depth: number
  cp?: number
  mate?: number
  bound?: 'lower' | 'upper'
  wdl?: [number, number, number]
  pvUci: string[]
  pvSan: string[]
}

export interface Analysis {
  type: 'analysis'
  gen: number
  depth: number
  nodes: number
  nps: number
  lines: AnalysisLine[]
}

type ServerMessage = Analysis | { type: 'error'; message: string }

/** Analysis connection that only delivers results for the latest request and resends it after reconnecting. */
export class AnalysisSocket {
  private ws?: WebSocket
  private gen = 0
  private request?: string
  private closed = false

  constructor(
    private onAnalysis: (analysis: Analysis) => void,
    private onError: (message: string) => void,
  ) {
    this.connect()
  }

  analyze(rootFen: string, moves: string[], multiPv: number) {
    this.request = JSON.stringify({ type: 'analyze', gen: ++this.gen, rootFen, moves, multiPv })
    this.send(this.request)
  }

  /** Generation of the latest `analyze` request; results for older ones are stale. */
  get generation() {
    return this.gen
  }

  options(threads: number, hashMb: number) {
    this.send(JSON.stringify({ type: 'options', threads, hashMb }))
  }

  stop() {
    this.request = undefined
    this.send('{"type":"stop"}')
  }

  close() {
    this.closed = true
    this.ws?.close()
  }

  private send(message: string) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(message)
  }

  private connect() {
    const ws = new WebSocket(`ws://${location.host}/ws`)
    ws.onopen = () => this.request && ws.send(this.request)
    ws.onmessage = (e) => {
      const message: ServerMessage = JSON.parse(e.data)
      if (message.type === 'error') this.onError(message.message)
      else if (message.gen === this.gen) this.onAnalysis(message)
    }
    ws.onclose = () => !this.closed && setTimeout(() => this.connect(), 1000)
    this.ws = ws
  }
}
