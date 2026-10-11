import { describe, expect, it } from 'vitest'
import { formatScore, whitePercent } from './eval'

const line = (score: { cp?: number; mate?: number; wdl?: [number, number, number] }) => ({ multiPv: 1, depth: 1, pvUci: [], pvSan: [], ...score })

describe('eval', () => {
  it('formats scores', () => {
    expect([34, -150, 0].map((cp) => formatScore(line({ cp })))).toEqual(['+0.34', '-1.50', '0.00'])
    expect([3, -2].map((mate) => formatScore(line({ mate })))).toEqual(['#3', '-#2'])
  })

  it('computes White expected score', () => {
    expect(whitePercent(line({ cp: 0, wdl: [100, 800, 100] }))).toBe(50)
    expect(whitePercent(line({ cp: 0, wdl: [600, 400, 0] }))).toBe(80)
    expect(whitePercent(line({ mate: -1 }))).toBe(0)
    expect(whitePercent(line({ cp: 0 }))).toBe(50)
    expect(whitePercent(line({ cp: 300 }))).toBeCloseTo(75.1, 1)
  })
})
