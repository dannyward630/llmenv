---
# No bare **/*.h: plain headers also match the C and C++ rule sets (three
# rulesets on one file); an ObjC project's real signal is its .m/.mm edits.
paths:
  - "**/*.m"
  - "**/*.mm"
---

# Objective-C Conventions

Distilled from [microsoft/objc-guide](https://github.com/microsoft/objc-guide).

## Naming

Names MUST follow this table.

| Entity | Convention |
|--------|------------|
| Class | Noun, three-letter prefix (`XYZObject`, not `Object` or two-letter `XYObject` — two-letter prefixes are reserved for system frameworks) |
| Category | `ClassName+XYZDescriptiveName.h`; methods/properties prefixed `xyz_` to avoid [name clashes](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/CustomizingExistingClasses/CustomizingExistingClasses.html) |
| Method | Starts lowercase, verb for actions, no `get` prefix on accessors, keyword before every argument describing that argument |
| Property | Noun/verb (`editable`) or, for adjectives, `getter=isEditable` |
| Instance variable | `_camelCase`, lowercase letter after the underscore (matches clang's implicit-synthesis naming and KVC ivar search) |
| Protocol | Gerund (`NSLocking`, not `NSLock`); delegate/data source protocols append `Delegate`/`DataSource` to the delegating class's name |
| Header file | Same name as the single class/protocol/category it declares |

```obj-c
@interface XYZObject : NSObject          // good: three-letter prefix, noun
@end

@interface Object : NSObject             // bad: no prefix
@end
```

## Prefer C Functions Over Categories

Categories replace methods on the class they extend; two categories implementing the same
selector produce undefined behavior at link time (which one wins is unspecified). SHOULD use a
C-style function instead, unless subclasses need to override the behavior.

```obj-c
NSString *XYZStringWithExcitement(NSString *string); // good: no runtime name-clash risk

@interface NSString (XYZExcitementAdditions)
- (NSString *)xyz_stringWithExcitement;              // bad: category method, collision-prone
@end
```

## Memory Management (ARC)

### Mechanics — what ARC is and is not

- ARC is **compile-time**: the compiler inserts `retain`/`release`/`autorelease` calls per the
  ownership rules below. It is **not** a garbage collector — no pauses, no heap scanning, and
  crucially **no cycle detection**. Deallocation is deterministic: an object dies the instant its
  last strong reference goes away. You MUST break reference cycles (see below).
- ARC manages **Objective-C objects and blocks only** (plus dispatch/XPC objects on modern
  deployment targets). It does **not** manage Core Foundation objects, `malloc`/`free` memory, or
  C++ `new`/`delete` — those stay manual even in `.m`/`.mm` files compiled with ARC.
- Ownership is inferred from **method-family naming**: methods in the `alloc`, `new`, `copy`,
  `mutableCopy`, and `init` families return a +1 (owned) reference; everything else returns +0.
  Family matching is word-based — `newspaperCount` is not in the `new` family. Consequence:
  MUST NOT name a method `initX…`/`newX…`/`copyX…` unless it actually returns an object the caller
  should own — if the convention must be broken, MUST annotate with `NS_RETURNS_RETAINED` /
  `NS_RETURNS_NOT_RETAINED` or `__attribute__((objc_method_family(none)))`.
- `init`-family methods **consume `self`**, and using the previous value of `self` after
  `[super init]` returns is undefined behavior — MUST write `self = [super init];` and work
  only with the result.

### MRC ownership attributes under ARC

An ownership attribute is a claim about a reference count. Under MRC the claim is advice the
compiler ignores; under ARC the compiler acts on it. A file that moves from MRC to ARC therefore
gains behavior it never had, and the attribute that was a no-op becomes live code.

- **MUST NOT write `__attribute__((ns_consumes_self))` in a file compiled with ARC.** ARC reads it
  as "this method releases `self` when it returns". A method that shows a window and expects the
  caller to keep the controller alive therefore frees it, and the window cannot outlive the call.
  The usual result is a window whose buttons do nothing: the controller was the nib's File's Owner
  and the action target, so every click goes to nil. There is no crash and no log line.
- **MUST NOT write `NS_RETURNS_RETAINED`, `NS_RETURNS_NOT_RETAINED`, or `NS_RETURNS_AUTORELEASED`
  in an ARC file** unless the method genuinely breaks the naming convention, and then MUST state
  the reason in a comment.
- Where an object MUST outlive the method that shows it, MUST hold an explicit strong reference —
  an ivar, a property, or a registry that the window's close clears. MUST NOT rely on the show
  method to keep its own receiver alive.
- When porting a file from MRC to ARC, MUST review every ownership attribute in it and MUST delete
  each one that ARC now covers. A leftover attribute is a bug that compiles.
- The same rule covers `-retain`, `-release`, `-autorelease`, and `[super dealloc]`. ARC refuses
  those outright, so they surface at build time; the attributes above do not.

Run the mechanical check before a commit that touches Objective-C, and in CI:

    bundles/objc-dev/skills/arc-ownership-check/scripts/check-arc-ownership.sh

It reports every ownership attribute that sits in a file compiled with ARC, reading the
`-fno-objc-arc` list from the project files so the MRC files do not raise false findings.

### Fast enumeration across a copy

A for-in loop calls `countByEnumeratingWithState:objects:count:` once per batch, and it treats
every call as the same collection. The object that answers owns the storage the loop reads.

- **MUST NOT implement `countByEnumeratingWithState:objects:count:` by forwarding to a computed
  collection**, such as `[self.containedObjects …]` where `-containedObjects` returns a copy.
  Each batch then runs against a different array. The batch cursor and the mutation guard belong
  to an array the loop does not hold, and the loop reads storage that nothing keeps alive.
- **MUST forward to the collection the object holds**, which is its own ivar.
- MUST give a copy a strong local if a copy is truly needed, so it lives for the whole loop.

This is the crash that shipped in AdiumY 2.0.1: adding a Jabber account died on the first roster
add, messaging a garbage pointer out of the contact list (#385). Both `AIListGroup` and
`AIMetaContact` forwarded their enumeration to a copy.

The same check reports it. `check-arc-ownership.sh` flags a `countByEnumeratingWithState:` body
that forwards to anything other than an ivar.

### Ownership qualifiers

| Qualifier | Semantics | Use for |
|-----------|-----------|---------|
| `__strong` | Owning reference (default for object pointers) | Normal references |
| `__weak` | Non-owning, **auto-zeroes** when the referent deallocates | Back-references, delegates, cycle breaking |
| `__unsafe_unretained` | Non-owning, **dangles** after dealloc (no zeroing) | Only for the few classes that forbid weak refs (`NSTextView`, `NSFont`, `NSColorSpace`, …) or a measured hot path where weak-load overhead matters |
| `__autoreleasing` | Object written back through an out-parameter | Almost never written by hand — `NSError **` parameters are implicitly this |

SHOULD default to `__strong` (by omission) and `__weak` for the back edge of any parent↔child or
delegate relationship. MUST reach for `__unsafe_unretained` only with a reason you can state.

### Property attributes

- **Object properties:** `strong` by default; `weak` for delegates/back-references; `copy` for
  `NSCopying` types (below). MUST NOT use `assign` for objects — it compiles but produces a
  non-zeroing dangling pointer, i.e. `__unsafe_unretained` by accident.
- **Scalars/structs** (`NSInteger`, `CGRect`, enums): `assign` (the only correct choice).
- **MUST use `copy`, not `strong`, for `NSCopying` properties** (`NSString`, `NSArray`, `NSDictionary`,
  blocks, ...). A `strong` property lets a caller hand you a mutable instance and mutate it out
  from under you after the fact; `copy` snapshots it (cheaply — copy-on-write for immutable
  inputs).

```obj-c
@property (nonatomic, copy) NSString *name;    // good: caller's mutable string can't change later
@property (nonatomic, strong) NSString *name;  // bad: aliasing hazard if caller passed NSMutableString
@property (nonatomic, weak) id<XYZWidgetDelegate> delegate; // good: breaks the cycle
```

### Retain cycles

ARC never collects cycles; every cycle is a leak until a reference is made weak or manually
nil'ed. MUST NOT create cycles. The recurring offenders:

- **Delegates / parent-child:** the back-reference MUST be `weak` (see Protocols below).
- **Blocks stored by `self`:** blocks capture `self` strongly by default. If `self` also stores
  the block (property, ivar, handler registry — directly or transitively), that's a cycle. MUST use
  the weak/strong dance:

```obj-c
__weak __typeof__(self) weakSelf = self;
self.completionHandler = ^{
    __strong __typeof__(weakSelf) strongSelf = weakSelf;
    if (strongSelf == nil) {
        return;
    }
    [strongSelf finish];   // strongSelf keeps self alive for the block body only
};
```

  Only stored blocks cycle. A one-shot escaping block that `self` does not retain
  (`dispatch_async`, `+[UIView animateWithDuration:…]`, an `NSURLSession` completion) may capture
  `self` strongly — the transient retain is usually *desired* (keep the object alive until the
  work finishes). MUST NOT cargo-cult `weakSelf` into those.
- **`NSTimer` / `CADisplayLink`:** target-based APIs retain their target, and a repeating timer
  holds it until `-invalidate`. MUST either invalidate deterministically (not in `dealloc` — dealloc
  never runs while the timer holds you) or use the block-based API with the weak/strong dance.
- **`NSNotificationCenter`:** block-based observation is **never** auto-unregistered — the
  center retains the block (and everything it captures) until you `-removeObserver:` the token. MUST remove the observer.
  Selector-based observers are safe to leave on modern targets (weakly referenced and
  auto-unregistered since iOS 9/macOS 10.11) but SHOULD remove them anyway for clarity.
- **`__block` captures:** under ARC, `__block` object variables are **retained** by the block
  (unlike MRC). `__block` is not a cycle-breaking tool; MUST use `__weak`.

### dealloc under ARC

- MUST NOT call `[super dealloc]` (compiler error) and MUST NOT release ivars — ARC emits both.
- REQUIRED by hand: invalidate timers/display links, remove KVO observers, close file
  descriptors, `free()` malloc'd ivars, `CFRelease` owned Core Foundation ivars.
- MUST NOT call accessors (`self.foo = nil;`) in `init` or `dealloc` — MUST use ivars directly. Accessors
  can fire KVO notifications and subclass overrides on a half-constructed/half-destroyed object.

### Core Foundation bridging

ARC does not manage CF objects; the CF ownership rules (own what `Create`/`Copy` returns,
`CFRelease` what you own) still apply. Crossing the ObjC↔CF boundary, MUST tell ARC who
owns the reference:

| Cast | Direction | Ownership effect |
|------|-----------|------------------|
| `__bridge` | Either | No transfer — both sides keep their existing responsibilities |
| `CFBridgingRetain()` (`__bridge_retained`) | ObjC → CF | +1 to you; you must `CFRelease` it later |
| `CFBridgingRelease()` (`__bridge_transfer`) | CF → ObjC | Hands your +1 to ARC; do **not** `CFRelease` |

SHOULD prefer the function spellings (`CFBridgingRetain`/`CFBridgingRelease`) over the cast keywords —
they're greppable and harder to typo into the wrong variant.

```obj-c
CFUUIDRef uuid = CFUUIDCreate(NULL);                              // +1, we own it
NSString *s = CFBridgingRelease(CFUUIDCreateString(NULL, uuid));  // ownership moved to ARC
CFRelease(uuid);                                                  // still ours to release
```

### Autorelease pools

SHOULD wrap loop bodies that create many temporary objects in `@autoreleasepool { }` — +0 returns
accumulate in the enclosing pool, which for a long loop means an unbounded high-water mark
until the runloop drains. SHOULD do the same at the top of long-lived background threads without a runloop.

### Lifetime sharp edge: interior pointers

ARC may release a local's object after its **last use**, not at end of scope. Taking an interior
pointer into an object's private storage and then using only the raw pointer can free the owner
mid-use under optimization. Foundation's own inner-pointer methods (`-[NSData bytes]`,
`-[NSString UTF8String]`) are annotated `NS_RETURNS_INNER_POINTER`, which extends the receiver's
lifetime to the last use of the pointer *within the calling function* — but not past a return or
an autorelease-pool drain. Rules:

- MUST annotate your own inner-pointer-returning methods with `NS_RETURNS_INNER_POINTER`.
- To pin an owner for a whole scope regardless of annotations, MUST mark the local
  `NS_VALID_UNTIL_END_OF_SCOPE` (aka `__attribute__((objc_precise_lifetime))`).
- MUST NOT store an interior pointer beyond the owner's guaranteed lifetime.

### When to use ARC — and when not

- **MUST use ARC everywhere, for all new code.** There is no performance case for new MRC code;
  ARC elides retain/release pairs the optimizer can prove redundant, and manual code re-creates
  the double-release/use-after-free bug class ARC eliminated.
- Legitimate exceptions are narrow and file-scoped (`-fno-objc-arc` per file in Build Phases):
  - Vendored legacy third-party sources that don't compile under ARC and aren't worth porting.
  - Runtime-level infrastructure that must manipulate retain counts directly (object caches,
    custom weak-reference machinery) — if you're not sure you're in this category, you aren't.
- Perceived ARC overhead in a measured hot path MUST be fixed **inside** ARC (hoist accessor calls,
  `__unsafe_unretained` locals, `@autoreleasepool`), not by opting the file out.
- MUST NOT mix models within a file; ARC files reject explicit `retain`/`release`/`autorelease`
  sends at compile time, and MRC files silently skip every guarantee described above.

## Object Creation

- **MUST use `instancetype`**, not `id`, as the return type of `alloc`/`init`/factory methods.
- **MUST have one designated initializer** per class, marked `NS_DESIGNATED_INITIALIZER`; MUST mark the
  inherited plain `init` `NS_UNAVAILABLE` if it shouldn't be called directly.

```obj-c
@interface XYZObject : NSObject
- (instancetype)initWithFoo:(Foo *)foo NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;
@end
```

## Nullability

MUST wrap headers in `NS_ASSUME_NONNULL_BEGIN` / `NS_ASSUME_NONNULL_END` and MUST mark the exceptions
`nullable`. Improves Swift interop and catches nil-misuse at compile time.

```obj-c
NS_ASSUME_NONNULL_BEGIN
@interface XYZList : NSObject
- (nullable XYZItem *)itemWithName:(NSString *)name;
@property (copy, nullable) NSString *name;
@end
NS_ASSUME_NONNULL_END
```

## Generics

SHOULD use lightweight generics (`NSArray<NSNumber *> *`) on collection properties, ivars, and
parameters. Types erase at runtime, but the compiler still catches wrong-type inserts at the
call site — the alternative is a silent `id` conversion that only fails, if ever, downstream.

## Syntax

- **MUST NOT use dot notation for method calls, including accessors.** MUST use message syntax (`[self frame]`,
  not `self.frame`). Dot syntax reads like a struct field access but is a full method invocation,
  and it hides [redundant re-invocations](#avoid-redundant-method-invocations) behind pleasant-looking syntax.
- **MUST brace single-line bodies** (`if`, `while`, `for`). An unbraced single statement has
  bitten real codebases hard — see the ["goto fail"](https://www.imperialviolet.org/2014/02/22/applebug.html) bug.
- **MUST NOT use implicit boolean conversion.** MUST compare explicitly: `if (object != nil)`, `if (count > 0)`,
  not `if (object)` or `if (count)`. `BOOL` values themselves are compared directly (`if (flag)`,
  not `if (flag == YES)`).

```obj-c
if (object != nil) { }   // good
if (object) { }          // bad: implicit pointer-to-bool

if (booleanStatement) {
    [self executeLogic]; // good: explicit scope
}
if (booleanStatement)
    [self executeLogic]; // bad: implicit scope, fragile under refactor
```

## Methods

- **SHOULD use a single return, as the last statement.** Multiple early returns duplicate any cleanup that
  needs to run on every path (notably paired Core Foundation `Create`/`Release` calls) and make
  it easy to forget one when adding a new exit point.
- **SHOULD avoid redundant method invocations.** SHOULD cache a repeatedly-used value (including property
  reads) in a local instead of re-sending the message at every use site — the implementation
  behind an accessor is not guaranteed cheap, and repeated sends have caused measurable
  regressions (e.g. against `NSProxy`-backed objects).

```obj-c
// bad: [self anObject] and [anObject button] each re-evaluated three times
[[[[self anObject] button] titleLabel] setFont:font];
[[[[self anObject] button] titleLabel] setLineBreakMode:mode];

// good
UILabel *buttonLabel = [[[self anObject] button] titleLabel];
[buttonLabel setFont:font];
[buttonLabel setLineBreakMode:mode];
```

## Headers

- **MUST use `#import`, not `#include`,** for Objective-C headers — `#import` already dedupes inclusion.
  MUST reserve `#include` for C/C++ headers that rely on include guards or preprocessor state
  (`#pragma push_macro`, X-macros).
- **SHOULD factor headers by audience** with a `+` suffix: `XYZObject+Protected.h` (subclass-only
  overrides), `XYZObject+Package.h` (module-internal), `XYZObject+Private.h` (unit-test-only).
  Each keeps the base header — and therefore the public API — minimal.

## Protocols

- **Delegates: optional methods only, held `weak`.** The delegating object MUST be fully
  functional with no delegate at all; a `strong` delegate reference is also a common source of
  retain cycles (the delegate often points back at the delegating object).
- **Data sources MAY have required methods** (the delegating object may be unable to function
  without them), but MUST keep the required set minimal and MUST hold the reference `weak` too.
- **MUST have one concept per protocol.** MUST NOT conflate unrelated behaviors (appearance + mouse handling)
  into one protocol just because one class happens to need both — split so classes that need
  only one aren't forced to stub the other.
- **SHOULD extract a protocol around a system API to unit test it** (e.g. wrap the slice of
  `NSFileManager` you call behind a protocol, inject a mock in tests).

## Documentation

MUST use Doxygen-style `///` comments on every documented entity (functions, interfaces, protocols,
properties, methods) in the header, describing all parameters and the return value. Xcode
generates and renders this format natively (⌥⌘/ inserts a stub).

```obj-c
/// Gets the initials of a person for an abbreviated representation in the UI.
///
/// @param person The person from which to return the initials.
/// @return The first character of \c firstName and \c lastName in locale-aware order.
NSString *XYZPersonGetInitials(XYZPerson *person);
```
