---
paths:
  - "**/*.ts"
  - "**/*.tsx"
---

# TypeScript Rules

## Compiler Strictness

MUST enable all in `tsconfig.json`:

```jsonc
"strict": true,
"noUncheckedIndexedAccess": true,
"exactOptionalPropertyTypes": true,
"noImplicitOverride": true,
"noPropertyAccessFromIndexSignature": true,
"verbatimModuleSyntax": true,
"isolatedModules": true
```

MUST use ESM only (`"type": "module"` in `package.json`). MUST NOT use CommonJS or `require`.

## Naming

| Style | Use for |
|-------|---------|
| `UpperCamelCase` | Classes, interfaces, types, enums, type parameters, React components |
| `lowerCamelCase` | Variables, parameters, functions, methods, properties |
| `CONSTANT_CASE` | Global constants, enum values, `static readonly` class properties |

- Acronyms MUST be treated as words: `loadHttpUrl`, not `loadHTTPURL`
- MUST NOT use a `_` prefix or suffix — use the `private` keyword
- MUST NOT use Hungarian notation or type-encoding (`strName`, `IFoo`, `EBar`)
- Names MUST be descriptive to a new reader. MUST NOT use vague abbreviations. MAY use single letters only in scopes <10 lines
- SHOULD avoid vague names (`Manager`, `Service`, `Handler`, `Processor`) when domain name exists

## Type Design

### No `any`

MUST NOT use `any`. MUST use `unknown` when type unknown, and MUST narrow with guards before use:

```typescript
// Bad
function parse(input: any): void { input.foo(); }

// Good
function parse(input: unknown): void {
  if (typeof input === "object" && input !== null && "foo" in input) {
    (input as { foo: () => void }).foo();
  }
}
```

If `any` is unavoidable (e.g., test mocks), MUST add an inline comment why.

### Type Assertions: Justify, Don't Chain

SHOULD use assertions (`as T`) sparingly, with clear comments. MUST NOT chain through `unknown` (`as unknown as T`) — defeats narrowing, triggers refactoring:

```typescript
// Bad — bypasses type system entirely
const agent = makeTestFixture() as unknown as Actor;

// Good — narrow with runtime check or add a proper helper
const agent = makeTestFixture();
if (!isValidActor(agent)) throw new Error("Invalid fixture");
// agent is now properly narrowed to Actor type

// Acceptable — minimal fixture, justified
// Type narrowing: fixture implements minimal Actor interface for roll execution.
// Full Actor type = 128+ Foundry properties unused here.
const agent = makeTestFixture() as unknown as Actor;
```

If writing `as unknown as`: (1) Type wrapper/constructor available? (2) Runtime check to narrow? (3) Fixture/model incomplete? MUST fix root cause. MUST NOT bypass type system.

### Interfaces vs Type Aliases

- SHOULD use `interface` for object shapes (better display, perf, extensibility)
- SHOULD use `type` for unions, intersections, tuples, mapped types, primitives

```typescript
// Object shapes → interface
interface User {
  name: string;
  email: string;
}

// Unions, tuples, primitives → type
type Result = Success | Failure;
type Pair = [string, number];
type UserId = string;
```

### Branded/Opaque Types

Domain identifiers SHOULD use branded types over primitives (like Rust newtypes):

```typescript
type UserId = string & { readonly __brand: unique symbol };
type TenantId = string & { readonly __brand: unique symbol };

function createUserId(raw: string): UserId {
  return raw as UserId;
}
```

Prevents mixing `UserId` and `TenantId`.

### Nullability

- SHOULD use `T | null` or `T | undefined` at point of use. SHOULD NOT bake into aliases
- SHOULD use optional (`?`) for omittable fields/params. SHOULD use `| undefined` for fields always present but may lack value
- SHOULD handle null near source. SHOULD NOT propagate through layers

### No Wrapper Types

MUST NOT use `String`, `Boolean`, `Number`, `Object`. MUST use lowercase primitives: `string`, `boolean`, `number`, `object`.

### Arrays

SHOULD use `T[]` for simple types. SHOULD use `Array<T>` for complex types (unions, objects):

```typescript
const names: string[];
const items: Array<string | number>;
```

### Enums

MUST use `enum`, not `const enum`. MUST include `default` or exhaustive check in switches.

### Generics

Every type param MUST be used. MUST NOT use phantom generics. SHOULD avoid return-type-only generics. MUST specify type arg explicitly.

### Prefer Simplest Type Construct

SHOULD avoid `Pick`, `Omit`, mapped, conditional types when spelling fields is simpler. Complex utility types hurt IDE support.

## Error Handling

- **MUST fail fast with context.** MUST throw `new Error("message")`. MUST NOT use bare `Error()`. SHOULD include operation, input, fix when possible
- **MUST NOT swallow exceptions.** Every `catch` MUST rethrow, log+rethrow, or handle. MUST NOT use an empty `catch {}`
- **MUST use `unknown` for caught errors.** TypeScript catch binds `unknown`. MUST narrow before accessing:

```typescript
try {
  await fetchData();
} catch (err: unknown) {
  const message = err instanceof Error ? err.message : String(err);
  throw new Error(`Failed to fetch data: ${message}`);
}
```

- **Callbacks ignoring errors MUST use `void` return type**, not `any`
- **MUST validate at system boundaries** (user input, API, files). MAY trust internal types

## Imports and Exports

- **MUST use named exports only.** MUST NOT use `export default`—inconsistent names, breaks find-references
- **MUST NOT use mutable exports.** MUST NOT use `export let`. MUST use getters if value changes
- **MUST NOT use container classes.** MUST NOT wrap statics in class for namespacing—export individually
- **MUST NOT use `import type`/`export type`.** TypeScript auto-distinguishes. Exception: `export type Foo = ...` is allowed
- **MUST NOT use namespace or `require`.** MUST use ESM `import`/`export` only
- **SHOULD destructure frequently used** (utils, framework). SHOULD use namespace imports (`import * as foo`) for large APIs

## Control Flow

### Strict Equality

MUST use `===` and `!==`. Exception: `== null` MAY be used to check both `null` and `undefined`.

### Exhaustive Switches

Every `switch` MUST have a `default` case. For discriminated unions, MUST use an exhaustiveness check:

```typescript
function assertNever(x: never): never {
  throw new Error(`Unexpected value: ${JSON.stringify(x)}`);
}

switch (action.type) {
  case "create": return handleCreate(action);
  case "delete": return handleDelete(action);
  default: return assertNever(action);
}
```

Non-empty cases MUST NOT fall through. Empty cases MAY group.

### No `forEach`

MUST NOT use `forEach` on arrays, sets, maps. It prevents early returns, breaks reachability analysis, and is harder to debug. MUST use `for...of`:

```typescript
// Bad
items.forEach((item) => { process(item); });

// Good
for (const item of items) {
  process(item);
}
```

### No `for...in`

MUST NOT use `for...in`—it iterates the prototype chain and string indices for arrays. MUST use `for...of`, `Object.keys()`, `Object.entries()`.

### Blocks Required

Multi-line control flow MUST use braces. Single-line MAY omit them: `if (done) return;`

## Variables

- MUST use `const` or `let`. MUST NOT use `var`
- SHOULD default to `const`. MUST use `let` when reassignment needed
- MUST NOT use before declaration

## Classes and Visibility

- **SHOULD minimize exported surface.** SHOULD export only what consumers need. SHOULD convert private methods to module functions
- **MUST NOT write `public`**—it's default. Exception: non-readonly constructor param properties
- **SHOULD use `readonly`** on properties not reassigned after construction
- **SHOULD use parameter properties** to avoid boilerplate
- **SHOULD initialize at declaration** when possible, eliminating the constructor
- **MUST NOT use `#private` fields.** MUST use `private` keyword—`#` causes size/perf regressions downleveled
- **Getters MUST be pure** (no side effects). SHOULD avoid trivial getter/setter pairs—make `readonly` or public
- **MUST NOT use arrow-function class properties** unless stable `this` reference needed (event handlers)

## Functions

- SHOULD use `function` for named (top-level, nested). Can't be reassigned
- SHOULD use arrows for callbacks, expressions
- Arrow callbacks ignoring return SHOULD use block body, not expression
- MUST NOT use `bind()` for handlers—creates unreferenceable temps. MUST use arrows or arrow properties
- MUST use semicolons—MUST NOT rely on ASI

## Magic Values

Hardcoded numbers, strings, timeouts MUST have a comment explaining *why*:

```typescript
// 30s matches server-side timeout
const POLL_TIMEOUT_MS = 30_000;
```

## Comments and Documentation

- **JSDoc (`/** */`)** MUST be used for public API—document purpose, not types (TypeScript has them)
- **Line comments (`//`)** SHOULD be for implementation notes only
- MUST NOT use `@param`/`@return` restating name/type. MAY add only beyond signature info
- MUST NOT use `@override`—not enforced, drifts
- MUST NOT commit commented-out code—delete. Git has history
- MUST place JSDoc before decorators, not between

## Coercion

- String: MUST use `String(x)` or templates. MUST NOT use `"" + x`
- Number: MUST use `Number(x)`, and MUST check `isNaN`. MUST NOT use unary `+`. MAY use `parseInt` only for non-base-10
- Boolean: implicit truthiness is allowed in conditionals. MUST NOT use `!!x` in `if`/`while`/`for`. Explicit checks are allowed (`arr.length > 0`)

## Spread

- MUST spread objects into objects, iterables into arrays only
- MUST NOT spread `null`, `undefined`, primitives
- MUST use conditional spread via ternary: `{ ...base, ...(condition ? extra : {}) }`, not `...(condition && extra)`

## Testing

Global testing philosophy (behavior not implementation, edges/errors, mock
boundaries only) applies — see AGENTS.md. TypeScript-specific:

- **SHOULD colocate tests.** `*.test.ts` next to source
- **MUST use `vitest`** runner

## Progressive Enhancement

Page rules: `web-stance.md`, `accessibility.md`, `htmx-accessibility.md`.

- MUST enhance server-rendered HTML with a custom element. MUST NOT hydrate or replace the markup.
- MUST NOT replace a native control with a scripted one.
- MUST announce a dynamic change through a live region that exists before the change.
- MUST move focus on purpose after a change. MUST NOT leave it on a removed element.
- SHOULD use `AbortController` to cancel a request that a newer request replaces.
- SHOULD format dates, numbers, and plurals with `Intl`.

## Anti-Patterns

| MUST NOT do this | MUST do this instead |
|---------------|-----------------|
| `any` | `unknown` + type narrowing |
| `export default` | Named exports |
| `var` | `const` / `let` |
| `.forEach()` | `for...of` |
| `for...in` | `for...of` / `Object.entries()` |
| `@ts-ignore` | Fix the type error |
| Type assertions (`as Foo`) without justification | Runtime type guards / narrowing |
| `as unknown as Foo` chains | Create type-safe wrappers or add proper narrowing — chaining through `unknown` bypasses the type system |
| Non-null assertions (`x!`) without justification | Null checks |
| `== ` / `!=` (except `== null`) | `===` / `!==` |
| `new String()` / `new Boolean()` / `new Number()` | Primitive literals |
| `Array()` constructor | Array literals `[]` |
| `namespace` | ES modules |
| `require()` | `import` |
| `debugger` | Remove before commit |
| Empty `catch {}` | Handle or rethrow |
| `console.log` in production code | Structured logging |
