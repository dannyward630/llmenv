---
paths:
  - "**/*.go"
  - "**/go.mod"
---

# Go Conventions

Distilled from the [Google Go Style Best Practices](https://google.github.io/styleguide/go/best-practices),
the JetBrains [10x Commandments of Highly Effective Go](https://blog.jetbrains.com/go/2025/10/16/the-10x-commandments-of-highly-effective-go/),
and [Leapcell's Go standards](https://dev.to/leapcell/go-coding-official-standards-and-best-practices-284k).

## Formatting & Linting

All code MUST be `gofmt`-formatted — not a style choice, a hard requirement enforced on every edit
(see `bundle.yaml`'s PostToolUse hook). MUST run `goimports` to manage imports. MUST run `go vet ./...` and
`staticcheck ./...` before committing; MUST treat findings as build failures, not suggestions.

## Naming

- Package names MUST be short, lowercase, no underscores or mixed caps (`httputil`, not `http_util`).
- Exported identifiers MUST be `CamelCase`. Unexported MUST be `camelCase`.
- Single-method interfaces SHOULD take an `-er` suffix named after the method: `Reader` for `Read`,
  `Writer` for `Write`.
- Getters SHOULD drop `Get`: `user.Name()`, not `user.GetName()`.
- **Shadowing:** `:=` inside a new scope (`if`, `for`) creates a *new* variable if any name on the
  left is new — code after the block still sees the outer one. This is a common source of "why
  didn't my reassignment take effect" bugs:
  ```go
  // Bad: ctx inside the if is shadowed; the outer ctx is untouched after the block.
  if *shortenDeadlines {
      ctx, cancel := context.WithTimeout(ctx, 3*time.Second)
      defer cancel()
  }
  // ctx here is still the original, unbounded one.
  ```
  MUST assign into the outer variable explicitly (`var err error; ctx, cancel = ...`) when the narrowed
  value must survive the block.

## Error Handling

- Errors are values. MUST check them with `if err != nil`. MUST return them as the last return value.
- MUST define sentinel errors with `errors.New` and compare with `errors.Is`. MUST NOT compare error
  strings. MUST NOT use `==` on a wrapped error.
- MUST wrap with `%w` (not `%v`) when the caller might need `errors.Is`/`errors.As` on the underlying
  error; MAY use `%v` when you're deliberately collapsing detail (e.g. translating a storage error into
  a domain error at an RPC boundary).
- MUST NOT `panic` for ordinary error conditions — reserve it for programmer errors (API misuse,
  invariant violations) that tests and code review should catch before production. Libraries
  MUST return errors; only `main` (or a top-level supervisor) decides whether a failure is fatal.
- MUST NOT `recover` just to avoid a crash — a panic recovered far from its origin can propagate
  corrupted state (held locks, half-written data) into code that has no idea it happened. A
  package MAY use `panic`/`recover` internally (e.g. a parser unwinding on a syntax error) only if
  the panic never crosses the package boundary.

## Concurrency

- SHOULD prefer no concurrency over concurrency you don't need — goroutines and shared state are where
  Go programs actually get hard to reason about.
- Every goroutine you start MUST have an explicit, visible way to stop: a `context.Context` it
  selects on, or a `sync.WaitGroup`/`errgroup.Group` the caller waits on. A goroutine with no exit
  condition is a leak.
  ```go
  var wg sync.WaitGroup
  wg.Go(task1)
  wg.Go(task2)
  wg.Wait()
  ```
  SHOULD use `errgroup.Group` when any task's failure should cancel the others.
- A channel parameter SHOULD be typed as send-only (`chan<-`) or receive-only (`<-chan`). It SHOULD NOT be
  bidirectional — bidirectional channel params are how a function accidentally deadlocks itself.
- MUST NOT share mutable package-level state across goroutines without a `sync.Mutex` or a
  single-owner "guard" goroutine reached via channel. MUST NOT rely on stdlib globals like
  `http.DefaultServeMux`/`http.DefaultClient` — an imported package can mutate them out from under
  you; MUST construct your own instance (`http.NewServeMux()`).

## Interfaces

- SHOULD accept interfaces as parameters and return concrete types — returning a concrete type lets the
  caller use its full API instead of being locked to the abstraction. The big exception is `error`
  and other cases where the interface itself *is* the contract you want to enforce (see below).
- SHOULD keep interfaces small. A large interface is harder to implement and harder to write generic code
  against; small interfaces compose.
- MUST NOT define an interface until a second implementation (or a test double) actually needs one.
  An interface with exactly one production implementation and no test-double use is speculative —
  MUST delete it and use the concrete type.
- Returning an interface (rather than the concrete type) is the right call when you want to hide
  methods the caller shouldn't touch — e.g. a type that implements `io.Reader` but also has an
  internal `Refill()` method should be returned as `io.Reader`, not `*ThrottledReader`, so callers
  can't reach in and break the invariant.

## Global State

Package-level mutable state is safe only when the package's *observable* behavior stays stateless
(a private cache the caller can't distinguish from a miss) or the state is logically constant.
It's unsafe the moment two independently-authored call paths can interact through it, or a test
needs to swap part of it out for a fake — that's a sign that state belongs in a struct passed
explicitly (constructor injection), not a package var.

## Function & Receiver Design

- MUST use a value receiver when the method doesn't mutate the receiver; MUST use a pointer receiver when it does, or
  when the type contains something that must not be copied (e.g. holds a `sync.Mutex`) — in that
  case every method on the type MUST take a pointer receiver, no exceptions.
- Multiple return values: `error` MUST be last.
- More than a couple of related parameters (especially several of the same type): SHOULD pass a struct
  instead of positional args — it's self-documenting at the call site and immune to
  argument-order bugs.
- SHOULD declare zero-value variables (`var coords Point`) rather than empty composite literals
  (`Point{X: 0, Y: 0}`) when the value is "ready for later use" (e.g. an unmarshal target). Maps
  are the exception: a nil map is safe to *read*, but writing to one panics — MUST initialize with
  `make` before mutating.

## Documentation

- Every exported identifier MUST get a doc comment starting with the identifier's own name — that's
  what godoc renders and what tooling expects.
- SHOULD NOT restate what's implied by convention: a `context.Context` parameter cancelling the
  function it's passed to doesn't need a sentence saying so. SHOULD document context behavior only when
  it's *not* the obvious default (e.g. the function swallows cancellation and returns a nil error,
  or has another shutdown path like a `Stop()` method).

## Testing

- Table-driven tests SHOULD be the default shape for anything with more than one interesting case;
  each case runs under `t.Run(tt.name, ...)` so failures report which case failed.
- Once a test table has more than a couple dozen lines, or has adjacent fields of the same type,
  MUST use field names in the struct literal — positional fields in a long table are a silent
  transposition bug waiting to happen.
- SHOULD reach for a custom `TestMain` only after ordinary per-test setup or a shared test helper proves
  insufficient — it's easy to get the setup/teardown ordering subtly wrong.

## Performance

Correctness first, then MUST profile with `pprof` before optimizing. MUST NOT guess. When a hot path's
final size actually is known ahead of time (e.g. converting a map to a slice), SHOULD preallocate with a
size hint (`make([]T, 0, n)`); SHOULD NOT do this reflexively on cold paths, it's clutter without a
measured benefit and can waste memory if the guessed size overshoots.

## Project Structure

SHOULD write packages, not programs: `main` SHOULD only parse flags/arguments and handle top-level
errors and cleanup; the real logic lives in an importable package that returns data and errors
rather than printing or calling `os.Exit`. SHOULD keep the module structure simple — start with one
package and split only when a real size or coupling problem shows up (see `go help modules`
layout guidance), not preemptively.
