import type { Chess } from 'chessops/chess'
import { ChildNode, defaultGame, defaultHeaders, makePgn, parsePgn, startingPosition, walk, type Game, type PgnNodeData } from 'chessops/pgn'
import { INITIAL_FEN, makeFen } from 'chessops/fen'
import { parseSan } from 'chessops/san'
import { MoveTree, type TreeNode } from './tree'

class Cursor {
  constructor(
    public node: TreeNode,
    public pos: Chess,
  ) {}
  clone() {
    return new Cursor(this.node, this.pos.clone())
  }
}

/** Parses the first game of a PGN. Throws on illegal moves or setups. */
export function importPgn(pgn: string): { tree: MoveTree; headers: Map<string, string> } {
  const [game] = parsePgn(pgn)
  if (!game) throw new Error('no game found')
  const pos = startingPosition(game.headers).unwrap()
  const tree = new MoveTree(makeFen(pos.toSetup()))
  tree.root.comments = game.comments
  let error: string | undefined
  walk(game.moves, new Cursor(tree.root, pos), (cursor, data) => {
    const move = parseSan(cursor.pos, data.san)
    const node = move && tree.play(cursor.node, move)
    if (!move || !node) {
      error ??= `illegal move: ${data.san}`
      return false
    }
    node.startingComments = data.startingComments
    node.comments = data.comments
    node.nags = data.nags
    cursor.pos.play(move)
    cursor.node = node
  })
  if (error) throw new Error(error)
  return { tree, headers: game.headers }
}

export function exportPgn(tree: MoveTree, headers = new Map<string, string>()): string {
  const game: Game<PgnNodeData> = defaultGame(() => (headers.size ? new Map(headers) : defaultHeaders()))
  if (tree.root.fen !== INITIAL_FEN) {
    game.headers.set('SetUp', '1')
    game.headers.set('FEN', tree.root.fen)
  }
  game.comments = tree.root.comments
  const copy = (from: TreeNode, to: { children: ChildNode<PgnNodeData>[] }) => {
    for (const child of from.children) {
      const { san, startingComments, comments, nags } = child
      const node = new ChildNode<PgnNodeData>({ san, startingComments, comments, nags })
      to.children.push(node)
      copy(child, node)
    }
  }
  copy(tree.root, game.moves)
  return makePgn(game)
}
