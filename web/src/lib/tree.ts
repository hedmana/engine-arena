import { Chess, castlingSide, normalizeMove } from 'chessops/chess'
import { INITIAL_FEN, makeFen, parseFen } from 'chessops/fen'
import { makeSanAndPlay } from 'chessops/san'
import { makeSquare, parseUci } from 'chessops/util'
import type { Move } from 'chessops/types'

export interface TreeNode {
  id: number
  /** Position after this node's move (the root's starting position). */
  fen: string
  /** Empty on the root. */
  san: string
  uci: string
  /** Full move number and side of the move, for display. */
  moveNumber: number
  white: boolean
  parent?: TreeNode
  /** First child is the main continuation. */
  children: TreeNode[]
  startingComments?: string[]
  comments?: string[]
  nags?: number[]
}

export const positionOf = (node: TreeNode): Chess => Chess.fromSetup(parseFen(node.fen).unwrap()).unwrap()

/** UCI as engines expect it: castling as king to g/c file, not king takes rook. */
const standardUci = (pos: Chess, move: Move): string => {
  if (!('from' in move)) throw new Error('drops are not supported')
  const side = castlingSide(pos, move)
  const to = side ? (move.from & 56) + (side === 'h' ? 6 : 2) : move.to
  return makeSquare(move.from) + makeSquare(to) + (move.promotion ? move.promotion[0] : '')
}

export class MoveTree {
  readonly root: TreeNode
  private nextId = 0

  constructor(fen: string = INITIAL_FEN) {
    const pos = Chess.fromSetup(parseFen(fen).unwrap()).unwrap()
    this.root = this.node({ fen: makeFen(pos.toSetup()), san: '', uci: '', moveNumber: pos.fullmoves, white: pos.turn === 'black', children: [] })
  }

  private node(fields: Omit<TreeNode, 'id'>): TreeNode {
    return { id: this.nextId++, ...fields }
  }

  /** Plays `move` after `parent`, reusing an existing child for the same move. Returns undefined if illegal. */
  play(parent: TreeNode, move: Move | string): TreeNode | undefined {
    const parsed = typeof move === 'string' ? parseUci(move) : move
    if (!parsed) return undefined
    const pos = positionOf(parent)
    const normalized = normalizeMove(pos, parsed)
    if (!pos.isLegal(normalized)) return undefined
    const uci = standardUci(pos, normalized)
    const existing = parent.children.find((c) => c.uci === uci)
    if (existing) return existing
    const moveNumber = pos.fullmoves
    const white = pos.turn === 'white'
    const san = makeSanAndPlay(pos, normalized)
    const child = this.node({ fen: makeFen(pos.toSetup()), san, uci, moveNumber, white, parent, children: [] })
    parent.children.push(child)
    return child
  }

  /** Moves from the root (exclusive) to `node` (inclusive). */
  static path(node: TreeNode): TreeNode[] {
    const path = []
    for (let n: TreeNode | undefined = node; n?.parent; n = n.parent) path.push(n)
    return path.reverse()
  }

  /** Follows main continuations from `node` to the end of its line. */
  static lineEnd(node: TreeNode): TreeNode {
    while (node.children.length) node = node.children[0]
    return node
  }

  /** Makes the variation containing `node` the main continuation one level up. */
  static promote(node: TreeNode): void {
    let n = node
    while (n.parent && n.parent.children[0] === n) n = n.parent
    const siblings = n.parent?.children
    if (!siblings) return
    siblings.splice(siblings.indexOf(n), 1)
    siblings.unshift(n)
  }

  /** Removes `node` and everything after it. Returns the node to show instead. */
  static delete(node: TreeNode): TreeNode {
    const parent = node.parent
    if (!parent) return node
    parent.children.splice(parent.children.indexOf(node), 1)
    return parent
  }
}
