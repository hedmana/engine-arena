import { describe, expect, it } from 'vitest'
import { eloDifference } from './elo'

describe('eloDifference', () => {
  it('matches reference values', () => {
    const cases: [number, number, number, number, number | undefined][] = [
      [60, 20, 20, 147.1907, 66.0134],
      [50, 0, 50, 0, 68.9888],
      [10, 30, 5, 38.764, 58.8072],
      [12, 88, 0, 41.8941, 22.4846],
      [1, 1, 0, 190.8485, undefined],
    ]
    for (const [w, d, l, elo, margin] of cases) {
      const result = eloDifference(w, d, l)!
      expect(result.elo).toBeCloseTo(elo, 3)
      if (margin === undefined) expect(result.margin).toBeUndefined()
      else expect(result.margin).toBeCloseTo(margin, 3)
    }
  })

  it('is symmetric', () => {
    expect(eloDifference(20, 20, 60)!.elo).toBeCloseTo(-147.1907, 3)
  })

  it('is undefined without games or at 0% and 100%', () => {
    expect(eloDifference(0, 0, 0)).toBeUndefined()
    expect(eloDifference(5, 0, 0)).toBeUndefined()
    expect(eloDifference(0, 0, 5)).toBeUndefined()
  })
})
