type Fn = (...args: never[]) => unknown

type NeverPlain = undefined | bigint | symbol | Fn

type AllTrue<Results> = false extends Results ? false : true

type Members<T> = { [K in keyof T]-?: Verdict<Required<T>[K]> }[keyof T]

// One verdict for each member of a union and each member of an object, so a single part that is not plain is enough to fail the whole.
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
