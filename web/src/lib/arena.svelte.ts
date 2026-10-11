import type { Key } from 'chessground/types'
import { Chess } from 'chessops/chess'
import { INITIAL_FEN, makeFen } from 'chessops/fen'
import { parseUci } from 'chessops/util'
import type { GameRecord, MatchState, MoveRecord } from './api'

type Event =
  | { type: 'state'; state: MatchState | null }
  | { type: 'match_start'; state: MatchState }
  | { type: 'game_start'; game: GameRecord }
  | { type: 'move'; game: number; move: MoveRecord; clocks: [number, number] }
  | { type: 'game_end'; game: GameRecord; wins: [number, number]; draws: number }
  | { type: 'match_end'; status: MatchState['status']; wins: [number, number]; draws: number }

/** Live match state from `/ws/matches`. Events mutate a working copy; the UI sees at most one update per frame. */
export class ArenaFeed {
  match = $state.raw<MatchState | null>(null)
  fen = $state.raw(INITIAL_FEN)
  lastMove = $state.raw<Key[] | undefined>()
  /** `performance.now()` when the side to move started thinking. */
  turnStarted = $state.raw(0)

  private state: MatchState | null = null
  private position = Chess.default()
  private frame = 0
  private ws?: WebSocket
  private closed = false

  constructor() {
    this.connect()
  }

  close() {
    this.closed = true
    cancelAnimationFrame(this.frame)
    this.ws?.close()
  }

  private connect() {
    const ws = new WebSocket(`ws://${location.host}/ws/matches`)
    ws.onmessage = (e) => this.apply(JSON.parse(e.data))
    ws.onclose = () => !this.closed && setTimeout(() => this.connect(), 1000)
    this.ws = ws
  }

  private apply(event: Event) {
    const s = this.state
    switch (event.type) {
      case 'state':
      case 'match_start':
        this.state = event.state
        this.replay(event.state?.current ?? null)
        break
      case 'game_start':
        if (s) s.current = event.game
        this.replay(event.game)
        break
      case 'move':
        if (s?.current) {
          s.current.moves.push(event.move)
          s.current.clocks = event.clocks
        }
        this.play(event.move.uci)
        break
      case 'game_end':
        if (s) {
          s.finished.push(event.game)
          s.current = null
          s.wins = event.wins
          s.draws = event.draws
        }
        break
      case 'match_end':
        if (s) {
          s.status = event.status
          s.wins = event.wins
          s.draws = event.draws
          s.current = null
        }
        break
    }
    this.turnStarted = performance.now()
    this.frame ||= requestAnimationFrame(() => this.flush())
  }

  private replay(game: GameRecord | null) {
    this.position = Chess.default()
    this.lastMove = undefined
    for (const move of game?.moves ?? []) this.play(move.uci)
  }

  private play(uci: string) {
    const move = parseUci(uci)
    if (!move || !('from' in move)) return
    this.position.play(move)
    this.lastMove = [uci.slice(0, 2) as Key, uci.slice(2, 4) as Key]
  }

  private flush() {
    this.frame = 0
    this.match = this.state && { ...this.state }
    this.fen = makeFen(this.position.toSetup())
  }
}
