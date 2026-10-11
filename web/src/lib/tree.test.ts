import { describe, expect, it } from 'vitest'
import { MoveTree } from './tree'

const line = (tree: MoveTree, ...moves: string[]) => moves.reduce((node, uci) => tree.play(node, uci)!, tree.root)

describe('MoveTree', () => {
  it('plays moves with SAN, standard castling UCI, and move numbers', () => {
    const tree = new MoveTree()
    const node = line(tree, 'e2e4', 'e7e5', 'g1f3', 'b8c6', 'f1c4', 'g8f6', 'e1h1')
    expect([node.san, node.uci, node.moveNumber, node.white]).toEqual(['O-O', 'e1g1', 4, true])
    expect(MoveTree.path(node).map((n) => n.san)).toEqual(['e4', 'e5', 'Nf3', 'Nc6', 'Bc4', 'Nf6', 'O-O'])
    expect(tree.play(node.parent!, 'e1g1')).toBe(node)
  })

  it('rejects illegal and malformed moves', () => {
    const tree = new MoveTree()
    expect(tree.play(tree.root, 'e2e5')).toBeUndefined()
    expect(tree.play(tree.root, 'zz')).toBeUndefined()
  })

  it('promotes and deletes variations', () => {
    const tree = new MoveTree()
    const e4 = line(tree, 'e2e4', 'e7e5')
    const c5 = tree.play(e4.parent!, 'c7c5')!
    const nf3 = tree.play(c5, 'g1f3')!
    MoveTree.promote(nf3)
    expect(e4.parent!.children[0]).toBe(c5)
    expect(MoveTree.lineEnd(tree.root)).toBe(nf3)
    expect(MoveTree.delete(c5)).toBe(e4.parent)
    expect(e4.parent!.children).toEqual([e4])
  })

  it('handles promotion and custom roots', () => {
    const tree = new MoveTree('8/P7/8/8/8/8/8/k6K w - - 0 60')
    const node = tree.play(tree.root, 'a7a8q')!
    expect([node.san, node.uci, node.moveNumber]).toEqual(['a8=Q+', 'a7a8q', 60])
  })
})
