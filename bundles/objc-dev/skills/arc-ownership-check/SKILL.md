---
name: arc-ownership-check
description: Use when finishing a change to Objective-C or Objective-C++ code, when porting MRC files to ARC, or when a window, dialog, or sheet appears but its buttons do nothing. Finds MRC ownership attributes (ns_consumes_self, NS_RETURNS_RETAINED, and friends) that sit in files compiled with ARC, where the compiler acts on them instead of ignoring them.
resources:
  - scripts/check-arc-ownership.sh
  - scripts/arc_ownership_check.py
---

# ARC ownership check

MUST run the script from the repo root.

```bash
"${CLAUDE_CONFIG_DIR}/skills/arc-ownership-check/scripts/check-arc-ownership.sh"
```

It reads the `-fno-objc-arc` file list out of the project files, so a file that genuinely needs
MRC does not raise a finding. It separates two kinds of result:

- An **error** is `ns_consumes_self`, or a `countByEnumeratingWithState:objects:count:` that
  forwards enumeration to a collection the object does not hold. Both are never correct in a file
  compiled with ARC, and the script exits 1. One error per line on stdout, as
  `path:line:what`.
- A **note** is a stated `NS_RETURNS_*` family. It is redundant when the name already matches,
  and a deliberate override when it does not. Notes go to stderr and do not fail the run.
  `--strict` makes them fail too.

## Why this matters

An ownership attribute is a claim about a reference count. Under MRC the compiler ignores the
claim. Under ARC the compiler acts on it. A file that moves from MRC to ARC therefore gains
behavior it never had, and the attribute that was a no-op becomes live code.

The worst of them is `__attribute__((ns_consumes_self))`. ARC reads it as "this method releases
`self` when it returns". A method that shows a window and expects the caller to keep the
controller alive therefore frees it. The controller was the nib's File's Owner and the action
target for its buttons, so every click goes to nil. The window stays on screen with no crash and
no log line, and nothing about it looks wrong.

## What to do with a finding

1. MUST delete the attribute when ARC covers the case, which is almost always.
2. MUST give the object an explicit owner when it has to outlive the method that shows it:
   an ivar, a property, or a registry that the window's close clears.
3. MAY keep the attribute only for a file that the project already builds with `-fno-objc-arc`.

## Checking a live app

A dead window is easier to confirm than to reason about. Launch the app, then read the live
window back:

```bash
lldb -b -p <pid> \
  -o 'expr -l objc++ -O -- (id)[[(id)[[(id)NSClassFromString(@"NSApplication") sharedApplication] valueForKey:@"windows"] objectAtIndex:0] valueForKey:@"delegate"]' \
  -o 'detach' -o 'quit'
```

A `nil` delegate on a window that is on screen means its controller is gone.
