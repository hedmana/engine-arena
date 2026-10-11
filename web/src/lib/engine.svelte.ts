import { AnalysisSocket, type Analysis } from './socket'

const CACHE_LIMIT = 5000

/** Engine output for the position on the board. The deepest result per position is cached so revisits show instantly. */
export class Engine {
  analysis = $state.raw<Analysis>()
  enabled = $state(true)
  multiPv = $state(3)
  threads = $state(Math.max(1, (navigator.hardwareConcurrency || 4) - 2))
  hashMb = $state(1024)
  error = $state('')

  private cache = new Map<string, Analysis>()
  private key = ''
  private incoming?: Analysis
  private frame = 0
  private socket = new AnalysisSocket(
    (analysis) => this.receive(analysis),
    (message) => (this.error = message),
  )

  /** Reads `enabled` and `multiPv`, so an effect calling this re-runs when they change. */
  analyze(rootFen: string, moves: string[], fen: string) {
    const { enabled, multiPv } = this
    this.key = `${multiPv} ${fen}`
    this.incoming = undefined
    this.analysis = enabled ? this.cache.get(this.key) : undefined
    if (enabled) this.socket.analyze(rootFen, moves, multiPv)
    else this.socket.stop()
  }

  /** Stops analysis while the board is not shown. */
  suspend() {
    this.incoming = undefined
    this.socket.stop()
  }

  configure() {
    this.socket.options(this.threads, this.hashMb)
  }

  close() {
    cancelAnimationFrame(this.frame)
    this.socket.close()
  }

  private receive(analysis: Analysis) {
    this.incoming = analysis
    this.frame ||= requestAnimationFrame(() => this.apply())
  }

  /** Applies at most one message per frame, never replacing a deeper cached result. */
  private apply() {
    this.frame = 0
    const fresh = this.incoming
    this.incoming = undefined
    if (!fresh || fresh.gen !== this.socket.generation) return
    const cached = this.cache.get(this.key)
    if (cached && cached.depth > fresh.depth) return
    this.cache.delete(this.key)
    this.cache.set(this.key, fresh)
    if (this.cache.size > CACHE_LIMIT) this.cache.delete(this.cache.keys().next().value!)
    this.analysis = fresh
  }
}
