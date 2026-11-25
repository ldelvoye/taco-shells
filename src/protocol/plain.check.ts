import type { Workspace } from './arrangement'
import type { Config } from './config'
import type { TacoShellsApi } from './ipc'
import type { Crossing, IsPlain } from './plain'
import type { Snapshot } from './snapshot'

type Assert<T extends true> = T

type Found<T> = [T] extends [never] ? false : true

// Nothing imports this file: typecheck compiles it, and a line here that stops holding is the failure.
// A message that stops being plain data fails on ApiIsPlain. ApiCrosses keeps that check honest:
// with nothing found to cross, it would pass whatever the API held.
export type ApiCrosses = Assert<Found<Crossing<TacoShellsApi>>>
export type ApiIsPlain = Assert<IsPlain<Crossing<TacoShellsApi>>>

export type PlainCases = [
  Assert<IsPlain<string>>,
  Assert<IsPlain<{ a: string; inner: { b?: number; c: boolean | null } }>>,
  Assert<IsPlain<[string, number]>>,
  Assert<IsPlain<{ id: string }[]>>,
  Assert<IsPlain<{ a: string } | { b: number }>>,
  Assert<IsPlain<Workspace>>,
  Assert<IsPlain<Snapshot>>,
  Assert<IsPlain<Config>>
]

export type NotPlainCases = [
  // @ts-expect-error
  Assert<IsPlain<Map<string, number>>>,
  // @ts-expect-error
  Assert<IsPlain<Set<string>>>,
  // @ts-expect-error
  Assert<IsPlain<Date>>,
  // @ts-expect-error
  Assert<IsPlain<() => void>>,
  // @ts-expect-error
  Assert<IsPlain<bigint>>,
  // @ts-expect-error
  Assert<IsPlain<{ a: string | undefined }>>,
  // @ts-expect-error
  Assert<IsPlain<{ deep: { rows: Map<string, number> } }>>,
  // @ts-expect-error
  Assert<IsPlain<string | Date>>
]
