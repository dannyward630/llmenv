---
paths:
  - "**/*.c"
  - "**/*.h"
---

# C Conventions

## Toolchain

| Purpose | Tool |
|---------|------|
| build | `cmake` or `make` |
| lint | `clang-tidy`, `cppcheck` |
| format | `clang-format` |
| sanitize | `-fsanitize=address,undefined` in debug builds |

MUST compile with `-std=c17 -Wall -Wextra -Wpedantic -Werror`. MUST fix every warning. MAY suppress only with
`// NOLINT(reason)` and a comment.

## Memory Management

C has no constructors or destructors — ownership is entirely manual. MUST make it explicit.

- **Every allocation MUST have a paired free in the same logical scope** where possible. If ownership
  transfers, MUST document it clearly in the function contract.
- **SHOULD use `malloc` sparingly.** Prefer storage with a fixed bound (stack or static) when the size is
  known. When the project uses a library with allocation helpers, SHOULD use those helpers instead:
  GLib `g_new0()`/`g_autofree`/`g_autoptr()`, APR or talloc pools, or Core Foundation allocators.
  GLib `g_malloc()`/`g_new()` abort on out-of-memory, so their result needs no NULL check.
- **MUST check every `malloc`/`calloc`/`realloc` return value** — MUST NOT dereference without a null check.
- **SHOULD prefer `calloc` over `malloc` + `memset`** for zero-initialized arrays — simpler and
  less error-prone.
- **MUST NOT use VLAs (variable-length arrays)** — undefined behavior on stack overflow; size unknown to
  the compiler for analysis. MUST use `malloc` or a fixed upper bound with a `static_assert`.

```c
// bad
int* buf = malloc(n * sizeof(int));
buf[0] = 1;                          // crash if malloc returned NULL

// good
int* buf = calloc(n, sizeof(int));
if (!buf) { return ENOMEM; }
```

## String & Buffer Safety

Buffer overflows are the most common C security vulnerability.

- **MUST use `strncpy` / `strncat` / `snprintf`** over `strcpy` / `strcat` / `sprintf`. SHOULD use
  `strlcpy` / `strlcat` (BSD) or `snprintf` for all string construction.
- **MUST NUL-terminate.** `strncpy` does not guarantee termination — MUST add explicit `buf[n-1] = '\0'`.
- **MUST know the difference** between buffer size (bytes allocated) and string length (`strlen` result).
  Off-by-one is endemic. MUST pass `sizeof(buf)` not `sizeof(buf) - 1` to `snprintf`.
- **MUST NOT use `gets()`** — removed from C11. MUST use `fgets(buf, sizeof(buf), stdin)`.

```c
char name[64];
snprintf(name, sizeof(name), "%s_%d", prefix, id);  // always safe
```

## `const` Correctness

- **MUST use `const` pointer parameters** for data that the function must not modify.
- **SHOULD use `const` globals** for lookup tables and compile-time constants. SHOULD prefer over `#define` for
  typed, scoped constants.

```c
size_t count_spaces(const char* s);   // s is not modified
```

## Integer Safety

- **MUST NOT mix signed and unsigned** in comparisons — implicit conversions cause silent bugs.
  MUST compile with `-Wsign-compare` (included in `-Wextra`).
- **MUST check for overflow** before arithmetic, not after. `a + b > MAX` is already UB if `a` and `b`
  are signed and overflow occurs.
- **MUST use `<stdint.h>` fixed-width types** (`uint32_t`, `int64_t`) for protocol fields, serialization,
  and any size-sensitive code. MUST NOT assume `int` or `long` width.
- **MUST use `size_t` for sizes and indices into memory.** Signed loop counters over `size_t` arrays cause
  signed/unsigned comparison warnings and potential wrapping.

```c
// bad: assumes int is 32-bit, implicit sign conversion
unsigned int len = strlen(s);
if (len - 1 > 0) { ... }    // always true if len==0 (wraps to UINT_MAX)

// good
size_t len = strlen(s);
if (len > 1) { ... }
```

## Error Handling

C has no exceptions. MUST be explicit and consistent within a module.

- **SHOULD return `int` status codes** (0 = success, negative = error) or `bool` (C99) for functions
  that can fail. MUST document the convention in the header.
- **`errno`** for POSIX/system calls — MUST check immediately after the call before any other
  call that might reset it.
- **MUST NOT silently ignore return values** of functions that can fail (`fclose`, `fwrite`, `munmap`,
  `pthread_mutex_lock`). MUST mark intentional discards with `(void)`.
- MUST provide context on error: include the operation, the input, and a corrective hint in error
  messages.

```c
if (fclose(fp) != 0) {
    fprintf(stderr, "fclose(%s): %s\n", path, strerror(errno));
    return -1;
}
```

## Header Conventions

- **Include guards** MUST be on every header — `#ifndef PROJECTNAME_MODULE_H` / `#define` / `#endif`.
  `#pragma once` is a common extension but not standard C.
- **Headers declare; `.c` files define.** MUST NOT put non-`inline`, non-`static` definitions in headers.
- **SHOULD forward-declare structs** instead of including headers when only a pointer is needed.
  Reduces compilation coupling.
- **`#include` order:** SHOULD put own header first, then standard library, then third-party. Keeps headers
  self-contained and exposes missing includes early.

```c
/* mymodule.h */
#ifndef MYPROJECT_MYMODULE_H
#define MYPROJECT_MYMODULE_H
/* ... */
#endif /* MYPROJECT_MYMODULE_H */
```

## Undefined Behavior — The Landmines

These constructs are silent UB in C. MUST NOT use them:

| Construct | Problem |
|-----------|---------|
| Signed integer overflow `a + b` | UB; compiler may assume it never happens |
| Null pointer dereference | UB even with volatile |
| Out-of-bounds array access | UB; no runtime check in C |
| Strict aliasing violation (`*(int*)float_ptr`) | Compiler reorders/eliminates loads |
| Using a freed pointer | UB after `free()` — set to `NULL` immediately |
| Uninitialized reads | Indeterminate value; UB in some cases |
| Shift by ≥ width (`x << 32` on `int32_t`) | UB |

MUST build with `-fsanitize=undefined,address` during development. UBSan catches most of
these at runtime.

## Concurrency (POSIX threads)

- **`pthread_mutex_t`** MUST be initialized before use (`PTHREAD_MUTEX_INITIALIZER` or `pthread_mutex_init`).
  MUST pair with `pthread_mutex_destroy`.
- **MUST NOT share global mutable state** across threads without a mutex.
- **`volatile` does not synchronize** between threads in C — MUST use `_Atomic` (C11) or explicit
  barriers for lock-free code.
- MUST check every `pthread_*` return value. SHOULD use a wrapper macro that aborts on failure in debug builds.

## What Not to Do

| MUST NOT use | MUST use instead |
|-------|-------------|
| `gets()` | `fgets()` |
| `strcpy` / `strcat` | `strlcpy` / `strlcat` / `snprintf` |
| `sprintf` | `snprintf` |
| VLAs | `malloc` + explicit size |
| `malloc` without null check | `malloc` + `if (!ptr) return ERR` |
| Signed/unsigned mixing | Consistent types + `-Wsign-compare` |
| `(void*)` casts to hide type errors | Fix the type |
| `volatile` for threading | `_Atomic` (C11) |
| Magic numbers | Named `const` or `#define` with units in name |
| Silent `errno` ignore | Check + log immediately after syscall |
