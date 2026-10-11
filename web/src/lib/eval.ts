import type { AnalysisLine } from './socket'

export function formatScore(line: AnalysisLine): string {
  if (line.mate !== undefined) return `${line.mate < 0 ? '-' : ''}#${Math.abs(line.mate)}`
  const pawns = line.cp! / 100
  return `${pawns > 0 ? '+' : ''}${pawns.toFixed(2)}`
}

/** White's expected score in percent: from WDL when available, else lichess's cp-to-win% curve. */
export function whitePercent(line: AnalysisLine): number {
  if (line.wdl) return (line.wdl[0] + line.wdl[1] / 2) / 10
  if (line.mate !== undefined) return line.mate > 0 ? 100 : 0
  return 50 + 50 * (2 / (1 + Math.exp(-0.00368208 * line.cp!)) - 1)
}
