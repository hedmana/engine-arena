const Z95 = 1.959963984540054

const toElo = (score: number) => -400 * Math.log10(1 / score - 1)

/**
 * Elo difference implied by a W/D/L record, with a 95% margin from the trinomial variance
 * of the per-game score. Undefined at 0% or 100%; the margin is undefined when the interval
 * reaches 0% or 100% (too few games).
 */
export function eloDifference(wins: number, draws: number, losses: number): { elo: number; margin?: number } | undefined {
  const games = wins + draws + losses
  const score = (wins + draws / 2) / games
  if (!games || score <= 0 || score >= 1) return undefined
  const variance = (wins * (1 - score) ** 2 + draws * (0.5 - score) ** 2 + losses * score ** 2) / games
  const delta = Z95 * Math.sqrt(variance / games)
  const [low, high] = [score - delta, score + delta]
  return { elo: toElo(score), margin: low > 0 && high < 1 ? (toElo(high) - toElo(low)) / 2 : undefined }
}
