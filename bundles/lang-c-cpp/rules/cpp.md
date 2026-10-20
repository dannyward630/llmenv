---
paths:
  - "**/*.cpp"
  - "**/*.cc"
  - "**/*.cxx"
  - "**/*.hpp"
  - "**/*.hxx"
  - "**/*.h"
---

# C++ Conventions (C++17 baseline; C++20/23 features marked)

## Toolchain

| Purpose | Tool |
|---------|------|
| build | `cmake` (3.15+) or `meson` |
| lint | `clang-tidy` |
| format | `clang-format` |
| sanitize | `-fsanitize=address,undefined` in debug builds |
| analyze | `cppcheck` |

MUST compile with at minimum: `-Wall -Wextra -Wpedantic -Werror`. MUST set `-std=` explicitly —
C++17 is the floor; anything below marked **(C++20)** or **(C++23)** needs that standard, so
MUST check the project's `-std=` before reaching for it (`std::span`/`std::jthread` won't compile
under C++17 — fall back to `gsl::span`/`std::thread` there).
MUST fix every warning — MAY suppress only with `// NOLINT(reason)` and a comment explaining why.

## Resource Management — RAII First

MUST NOT manage resources manually. Every resource (memory, file, socket, lock) MUST have an owner
with automatic cleanup.

- **MUST NOT use raw `new`/`delete`** — MUST use `std::make_unique<T>()` and `std::make_shared<T>()`.
- **SHOULD use `malloc`/`free` sparingly.** They skip constructors and destructors, so MUST NOT use them
  for C++ objects. In C-interop code, SHOULD prefer the C library's own allocation helpers when it has
  them (GLib `g_new0()`/`g_autoptr()`, Core Foundation allocators), and SHOULD hold the result in a
  `std::unique_ptr` with a custom deleter.
- SHOULD prefer `unique_ptr` over `shared_ptr` unless you genuinely need shared ownership; `shared_ptr`
  has reference-count overhead and makes lifetime reasoning harder.
- MUST use `std::lock_guard` / `std::scoped_lock` for mutexes. MUST NOT use manual `lock()`/`unlock()`.

```cpp
// bad
int* p = new int[100];
// ...
delete[] p;

// good
auto p = std::make_unique<int[]>(100);
```

## Type Safety

- **`const` by default** — every variable, parameter, and member function that can be `const`
  SHOULD be. `const&` for input parameters of non-trivial types.
- **MUST NOT use C-style casts** `(T)x` — MUST use `static_cast`, `reinterpret_cast` (MUST justify in a comment),
  `const_cast` (SHOULD NOT). Each cast is explicit about what it does.
- **SHOULD use `string_view` over `const char*` or `const string&`** for read-only string parameters (C++17).
  Zero-cost abstraction that accepts both `std::string` and string literals.
- **SHOULD prefer `enum class`** over plain `enum` — scoped, no implicit int conversion.
- **SHOULD use `[[nodiscard]]`** on functions whose return value must be checked (error codes, resource handles).

```cpp
void process(std::string_view name);          // accepts string and char* callers
[[nodiscard]] std::error_code write(Span s);  // caller must check
```

## Bounds Safety — No Pointer Arithmetic

Raw pointer arithmetic is undefined behavior waiting to happen.

- **MUST use `std::span<T>`** (C++20) or `gsl::span<T>` instead of `(T*, size_t)` pairs.
- **MUST NOT index past bounds.** SHOULD prefer range-`for` or algorithms over manual indexing.
- **MUST NOT use array-to-pointer decay** at function boundaries — MUST accept `span`, not `T*`.
- SHOULD use `.at()` for bounds-checked access in non-performance-critical paths.

```cpp
// bad
void process(int* data, size_t len);

// good
void process(std::span<const int> data);
```

## Functions & Interfaces

- **SHOULD return values instead of using out-parameters.** SHOULD use structured bindings or `std::pair`/`struct` for
  multiple returns. C++17 NRVO makes return-by-value free for local objects.
- **MUST use ≤5 parameters** — beyond that, MUST introduce a parameter struct.
- **Pure virtual interfaces** — SHOULD prefer empty abstract base classes (no data members) over
  concrete base classes. Avoids the fragile base class problem.

```cpp
// bad
void find(const vector<int>& v, vector<int*>& out_matches, int x);

// good
std::vector<int*> find(const std::vector<int>& v, int x);
```

## Error Handling

MUST choose one strategy per module and be consistent:

| Context | Strategy |
|---------|----------|
| General application code | Exceptions + RAII |
| Performance-critical / embedded | `std::expected<T,E>` (C++23) or `std::optional<T>` + error enum |
| Constructor failure | Throw — there is no "return" from a constructor |

- **`noexcept`** SHOULD be on functions that cannot throw (move operations, destructors, pure arithmetic).
  Allows compiler optimisations and is required for strong exception safety of containers.
- **MUST NOT `throw` in a destructor.** Destructors MUST be `noexcept`.
- **MUST NOT swallow exceptions silently** — at minimum log + rethrow.
- Constructors establish invariants. If the invariant cannot be established, MUST throw. MUST NOT create
  zombie objects that require an `init()` call.

```cpp
std::string to_upper(std::string s) noexcept;   // cannot throw
```

## C++17 Features — Use Them

SHOULD use these features where they fit.

### Structured Bindings
```cpp
auto [iter, inserted] = my_map.emplace(key, value);
if (!inserted) { /* key already existed */ }
```

### `if` / `switch` with Init Statement
```cpp
if (auto it = m.find(key); it != m.end()) {
    use(it->second);
}
```

### `std::optional<T>`
SHOULD replace sentinel values (`-1`, `nullptr`, `""`) and boolean out-parameters with `optional`.
```cpp
std::optional<Config> load_config(std::string_view path);
```

### `std::variant<Ts...>` + `std::visit`
Type-safe tagged unions. SHOULD prefer over `union` + manual tag.
```cpp
using Result = std::variant<Value, ParseError>;
std::visit(overloaded{
    [](Value v)      { use(v); },
    [](ParseError e) { report(e); },
}, result);
```

### `std::filesystem`
```cpp
#include <filesystem>
namespace fs = std::filesystem;
fs::path p = fs::current_path() / "data" / "config.json";
```

### `if constexpr`
SHOULD replace SFINAE and `enable_if` for compile-time branching inside templates.
```cpp
template<typename T>
void serialize(T val) {
    if constexpr (std::is_integral_v<T>) { write_int(val); }
    else                                 { write_float(val); }
}
```

### Fold Expressions (variadic templates)
```cpp
template<typename... Args>
bool all_positive(Args... args) { return (... && (args > 0)); }
```

### Class Template Argument Deduction (CTAD)
```cpp
std::pair p{1, 2.0};     // deduced pair<int, double>
std::vector v{1, 2, 3};  // deduced vector<int>
```

## `override` and `final`

- **MUST mark overriding virtual functions `override`** — the compiler will error if the
  signature drifts from the base, catching silent non-override bugs.
- **SHOULD use `final`** on classes or virtual functions where further derivation is not intended. Enables
  devirtualization optimisations and makes intent explicit.
- **MUST NOT re-declare `virtual` on an override** — `override` implies virtual; spelling both is
  redundant noise.

```cpp
struct Base {
    virtual void draw() = 0;
    virtual ~Base() = default;
};

struct Widget final : Base {
    void draw() override;   // good — override, not virtual
};
```

## Classes — Rule of Zero / Five

**Rule of Zero:** SHOULD prefer classes with no user-defined destructor, copy, or move operations.
Let member types (smart pointers, containers) handle resource cleanup automatically.

**Rule of Five:** If you define *any* of destructor, copy-ctor, copy-assign, move-ctor, move-assign
— MUST define or `=delete` *all five*. Partial definitions cause subtle bugs.

```cpp
struct Buffer {
    std::vector<std::byte> data;  // Rule of Zero: vector handles everything
};
```

- **Polymorphic base classes** MUST have a `virtual` destructor (or `protected` non-virtual).
- MUST mark move operations `noexcept` — containers check this to choose move vs. copy.
- SHOULD prefer `= default` to writing trivial implementations.

## Concurrency

- **`volatile` does not synchronize.** MUST use `std::atomic<T>` for shared variables across threads.
- **MUST use `std::mutex` + `std::lock_guard`** for critical sections. `std::scoped_lock` (C++17) for
  multiple mutexes at once — avoids deadlock via internal acquisition ordering.
- SHOULD prefer `std::jthread` (C++20) over `std::thread` — auto-joins on destruction, carries `stop_token`.
- **MUST NOT use raw `lock()`/`unlock()`** — MUST use RAII wrappers so exceptions can't leak a locked mutex.

```cpp
std::atomic<int> counter{0};   // correct shared counter
// NOT: volatile int counter = 0;
```

For full threading API detail — condition variables, futures, parallel STL, C++20 latches/barriers,
memory model (acquire/release/relaxed), thread-safe initialization:
MUST load the `cpp-concurrency` skill.

## Source File Conventions

- **`#pragma once`** is acceptable and portable enough for modern toolchains. If strict ISO C++
  portability matters, MUST use `#ifndef PROJECTNAME_PATH_FILE_H` guards instead.
- **Headers declare; `.cpp` files define.** MUST NOT put non-`inline`, non-`constexpr` definitions in headers
  — causes ODR violations.
- **SHOULD forward-declare instead of including** where the full type is not needed (pointer/reference
  parameters). Reduces compile-time coupling.
- SHOULD order includes: own header → standard library → third-party → project. Keeps header
  self-containment visible.

## Hot-Path / Real-Time Safety

In any tight loop where latency or jitter matters (audio callbacks, game tick, interrupt handlers,
lock-free queues), the code MUST follow these rules:

- **MUST NOT allocate** — `malloc`/`new`/container growth all potentially block on the OS allocator.
  MUST pre-allocate at init time; MUST use fixed-size ring buffers or pre-allocated pools at runtime.
- **MUST NOT lock** — `std::mutex::lock()` can block indefinitely. MUST use lock-free queues
  (`std::atomic`, SPSC ring buffer) for hot-path cross-thread communication.
- **MUST NOT do blocking I/O** — no file reads, no sockets, no `printf` (on some platforms it acquires
  a lock). MUST use a separate logging thread with a lock-free queue.
- **MUST NOT use exceptions in hot paths** — exception throwing/catching has non-trivial overhead on some
  ABIs. `noexcept` lets the compiler generate tighter code.
- **SHOULD avoid virtual dispatch in inner loops** — if the vtable lookup cost is measured to matter,
  use templates/CRTP or devirtualize with `final`.

```cpp
// audio/realtime callback — everything here must be allocation-free
void ProcessBlock(double** inputs, double** outputs, int nFrames) noexcept {
    for (int i = 0; i < nFrames; ++i) {
        outputs[0][i] = mFilter.Process(inputs[0][i]);  // pre-allocated state
    }
    mSender.TransmitData(*this, mMeterData);  // lock-free queue to UI thread
}
```

## What Not to Do

| MUST NOT use | MUST use instead |
|-------|-------------|
| `malloc` / `free` for C++ objects | `make_unique` / `make_shared` / containers |
| Raw `new` / `delete` | RAII types |
| `char*` for strings | `std::string` / `std::string_view` |
| C-style arrays `T arr[]` | `std::array<T,N>` / `std::vector<T>` / `std::span<T>` |
| `union` for variant data | `std::variant` |
| `void*` | templates or virtual dispatch |
| `(T)x` casts | `static_cast<T>(x)` |
| `volatile` for threading | `std::atomic<T>` |
| `unsigned` to avoid negatives | Signed with bounds checks |
| Output parameters `T&` | Return values / `std::optional` |
| Pointer arithmetic | `std::span` + iterators |
| Bare `catch (...)` swallow | Log + rethrow |
