import type { Workspace } from './arrangement'
import type { Config } from './config'
import type { TacoShellsApi } from './ipc'
import type { Snapshot } from './snapshot'

type Fn = (...args: never[]) => unknown

type NeverPlain = undefined | bigint | symbol | Fn

type AllTrue<Results> = false extends Results ? false : true

type Members<T> = { [K in keyof T]-?: Verdict<Required<T>[K]> }[keyof T]

type Verdict<T> = T extends string | number | boolean | null
  ? true
  : T extends NeverPlain
    ? false
    : T extends readonly (infer Element)[]
      ? AllTrue<Verdict<Element>>
      : T extends object
        ? AllTrue<Members<T>>
        : false

/** True when a value of this type comes back unchanged from JSON.stringify then JSON.parse. */
export type IsPlain<T> = AllTrue<Verdict<T>>

type ListenerCrossing<Listener> = Listener extends (...args: infer Params) => unknown
  ? { [Index in keyof Params]-?: Params[Index] }[number]
  : never

type ParameterCrossing<Parameter> = Parameter extends Fn ? ListenerCrossing<Parameter> : Parameter

type ResultCrossing<Result> = Exclude<Awaited<Result>, void | Fn>

type FunctionCrossing<Member> = Member extends (...args: infer Params) => infer Result
  ? { [Index in keyof Params]-?: ParameterCrossing<Params[Index]> }[number] | ResultCrossing<Result>
  : never

type MemberCrossing<Member> = Member extends Fn ? FunctionCrossing<Member> : Member

type GroupCrossing<Group> = { [Name in keyof Group]-?: MemberCrossing<Group[Name]> }[keyof Group]

/** Every type that crosses a bridge API made of groups of members: parameters, a listener's own parameters, results, and members that are data. */
export type Crossing<Api> = { [Group in keyof Api]-?: GroupCrossing<Api[Group]> }[keyof Api]

type Assert<T extends true> = T

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

type Found<T> = [T] extends [never] ? false : true

// A message that stops being plain data fails typecheck on the second line. The first
// keeps that check honest: with nothing found to cross, it would pass whatever the API held.
export type ApiCrosses = Assert<Found<Crossing<TacoShellsApi>>>
export type ApiIsPlain = Assert<IsPlain<Crossing<TacoShellsApi>>>
