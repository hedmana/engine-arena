import { describe, expect, it } from 'vitest'
import { exportPgn, importPgn } from './pgn'
import { MoveTree } from './tree'

const PGN = `[Event "Casual"]
[Site "?"]
[Date "????.??.??"]
[Round "?"]
[White "A"]
[Black "B"]
[Result "*"]

{ Opening } 1. e4 e5 2. Nf3 $1 { Main } ( 2. Nc3 Nf6 ( 2... Nc6 3. f4 ) 3. f4 ) 2... Nc6 3. Bb5 { Ruy } ( { Italian instead } 3. Bc4 Bc5 ) 3... a6 *
`

describe('PGN', () => {
  it('round-trips variations, comments, and NAGs losslessly', () => {
    const { tree, headers } = importPgn(PGN)
    expect(exportPgn(tree, headers)).toBe(PGN)
  })

  it('round-trips a custom starting position', () => {
    const tree = new MoveTree('4k3/8/8/8/8/8/4P3/4K3 w - - 0 1')
    tree.play(tree.root, 'e2e4')
    const again = importPgn(exportPgn(tree)).tree
    expect(again.root.fen).toBe(tree.root.fen)
    expect(MoveTree.lineEnd(again.root).san).toBe('e4')
  })

  it('rejects illegal moves', () => {
    expect(() => importPgn('1. e4 e5 2. Ke3 *')).toThrow('illegal move: Ke3')
  })
})
