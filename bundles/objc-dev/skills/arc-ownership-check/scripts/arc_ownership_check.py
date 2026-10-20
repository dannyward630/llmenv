"""Find MRC ownership attributes in files compiled with ARC.

An ownership attribute is a claim about a reference count. Under MRC the compiler
ignores it. Under ARC the compiler acts on it, so a file that moved from MRC to ARC
gains behavior it never had. See bundles/objc-dev/rules/objc.md, section
"MRC ownership attributes under ARC".

A file that the project compiles with -fno-objc-arc is expected to carry these
attributes, so it is not a finding. The flag is read from the project files.
"""

from __future__ import annotations

import argparse
import os
import plistlib
import re
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path

# An attribute that is never correct in a file compiled with ARC. It makes the
# method release its own receiver, so a window controller that is shown and kept
# alive only by the caller is freed the moment the show method returns.
ERROR_ATTRIBUTES = ("ns_consumes_self",)
ATTRIBUTE_ERROR = "ns_consumes_self"

# A for-in loop calls this once per batch and treats every call as the same
# collection, so forwarding to a computed collection leaves the loop reading
# storage that nothing owns.
ENUMERATION_METHOD = re.compile(
    r"-\s*\(NSUInteger\)\s*countByEnumeratingWithState:.*?\n\{", re.DOTALL
)
FORWARD = re.compile(r"return\s*\[\s*(?P<receiver>[^\]\n]+?)\s+countByEnumeratingWithState:")
IVAR = re.compile(r"_?[A-Za-z][A-Za-z0-9_]*")

# Attributes that state the method family. Under ARC the family is inferred from
# the name, so a correct one is redundant and an incorrect one is a deliberate
# override. Worth reading, not worth failing on.
NOTE_ATTRIBUTES = (
    "NS_RETURNS_RETAINED",
    "NS_RETURNS_NOT_RETAINED",
    "NS_RETURNS_AUTORELEASED",
)

ATTRIBUTE_PATTERN = re.compile("|".join(ERROR_ATTRIBUTES + NOTE_ATTRIBUTES))
SOURCE_SUFFIXES = {".h", ".m", ".mm"}

# Directories that hold generated or vendored copies, never the project's own sources.
SKIPPED_DIRECTORY_NAMES = {
    ".git",
    ".build",
    "DerivedData",
    "build",
    "node_modules",
}


class Scan:
    """What one run learned: the MRC files, and each file it could not read."""

    def __init__(self) -> None:
        self.mrc_paths: set[Path] = set()
        # Basenames of MRC references whose full path cannot be worked out.
        self.mrc_names: set[str] = set()
        self.read_failures = 0

    def failed(self, message: str) -> None:
        print(f"arc-ownership-check: {message}", file=sys.stderr)
        self.read_failures += 1

    def is_mrc(self, path: Path) -> bool:
        return path.resolve() in self.mrc_paths or path.name in self.mrc_names


def walk(root: Path) -> Iterator[Path]:
    """Yield every file under root, skipping generated and vendored trees."""
    for directory, subdirectories, names in os.walk(root):
        subdirectories[:] = [name for name in subdirectories if name not in SKIPPED_DIRECTORY_NAMES]
        for name in names:
            if name.startswith(".!"):
                continue  # A project file Xcode left half-written
            yield Path(directory) / name


def read_project_file(project_file: Path, scan: Scan) -> dict:
    """Return the objects of a project file, through plutil so both layouts parse."""
    try:
        xml = subprocess.run(
            ["plutil", "-convert", "xml1", "-o", "-", str(project_file)],
            check=True,
            capture_output=True,
        ).stdout
        return plistlib.loads(xml).get("objects", {})
    except FileNotFoundError:
        scan.failed("plutil not found, so no project file can be read. Run on macOS.")
    except subprocess.CalledProcessError as error:
        detail = error.stderr.decode("utf-8", "replace").strip()
        scan.failed(f"cannot read {project_file}: {detail}")
    except plistlib.InvalidFileException as error:
        scan.failed(f"cannot parse {project_file}: {error}")
    return {}


def _group_parents(objects: dict) -> dict[str, str]:
    """Map each object id to the id of the group that lists it as a child."""
    parents: dict[str, str] = {}
    for group_id, entry in objects.items():
        if isinstance(entry, dict):
            for child in entry.get("children") or []:
                parents[child] = group_id
    return parents


def _reference_path(
    ref_id: str, objects: dict, parents: dict[str, str], source_root: Path
) -> Path | None:
    """The full path of a file reference, or None for a tree this check cannot place."""
    parts: list[str] = []
    current: str | None = ref_id
    while current is not None:
        entry = objects.get(current, {})
        if entry.get("path"):
            parts.append(entry["path"])
        tree = entry.get("sourceTree", "<group>")
        if tree == "<absolute>":
            return Path(*reversed(parts))
        if tree == "SOURCE_ROOT":
            return source_root.joinpath(*reversed(parts))
        if tree != "<group>":
            return None  # BUILT_PRODUCTS_DIR, SDKROOT, and the like
        current = parents.get(current)
    return source_root.joinpath(*reversed(parts))


def add_mrc_files(project_file: Path, scan: Scan) -> None:
    """Record the files this project compiles with -fno-objc-arc."""
    objects = read_project_file(project_file, scan)
    parents = _group_parents(objects)
    source_root = project_file.parent.parent  # the folder that holds Name.xcodeproj
    for entry in objects.values():
        if not isinstance(entry, dict) or entry.get("isa") != "PBXBuildFile":
            continue
        if "-fno-objc-arc" not in entry.get("settings", {}).get("COMPILER_FLAGS", ""):
            continue
        ref_id = entry.get("fileRef", "")
        path = _reference_path(ref_id, objects, parents, source_root)
        if path is not None:
            scan.mrc_paths.add(path.resolve())
            continue
        name = objects.get(ref_id, {}).get("path") or objects.get(ref_id, {}).get("name")
        if name:
            print(
                f"arc-ownership-check: cannot place {name}; every file of that name counts as MRC",
                file=sys.stderr,
            )
            scan.mrc_names.add(Path(name).name)


def collect_findings(root: Path, scan: Scan) -> Iterator[tuple[bool, str]]:
    """Yield (is error, "path:line:text") for every finding in a file compiled with ARC."""
    for path in walk(root):
        if path.suffix not in SOURCE_SUFFIXES or scan.is_mrc(path):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            scan.failed(f"cannot read {path}: {error}")
            continue
        for number, line in enumerate(text.splitlines(), start=1):
            match = ATTRIBUTE_PATTERN.search(line)
            if match:
                yield (
                    match.group(0) in ERROR_ATTRIBUTES,
                    f"{path}:{number}:{match.group(0)}",
                )
        for number, receiver in enumeration_forwarding(text):
            yield True, f"{path}:{number}:forwards enumeration to {receiver}"


def returns_a_copy(text: str, name: str) -> bool:
    """True when this file defines -name and its body copies rather than returns an ivar."""
    definition = re.search(rf"-\s*\([^)]*\)\s*{re.escape(name)}\b[^\n]*\n\{{", text)
    if definition is None:
        return False
    body = text[definition.end() : text.find("\n}", definition.end())]
    return "copy]" in body


def enumeration_forwarding(text: str) -> Iterator[tuple[int, str]]:
    """Yield (line number, receiver) for each fast-enumeration forward to a computed collection."""
    for method in ENUMERATION_METHOD.finditer(text):
        body = text[method.end() : text.find("\n}", method.end())]
        forward = FORWARD.search(body)
        if forward is None:
            continue
        receiver = forward.group("receiver").strip()
        if IVAR.fullmatch(receiver):
            continue
        # self.name is fine when this file defines -name to hand back the array it holds.
        # It is the copy that hurts, so only report the property when it copies.
        property_name = re.fullmatch(r"self\.([A-Za-z][A-Za-z0-9_]*)", receiver)
        if property_name and not returns_a_copy(text, property_name.group(1)):
            continue
        line = text.count("\n", 0, method.start() + forward.start()) + 1
        yield line, receiver


def report(scan: Scan, root: Path, strict: bool) -> int:
    """Print every finding, and return the exit code."""
    errors = 0
    notes = 0
    for is_error, finding in collect_findings(root, scan):
        if is_error:
            print(finding)
            errors += 1
        else:
            print(f"arc-ownership-check: note: {finding}", file=sys.stderr)
            notes += 1
    if notes > 0:
        print(
            f"arc-ownership-check: {notes} note(s): a stated method family. "
            "Check each one against the name, then leave it or delete it.",
            file=sys.stderr,
        )
    if scan.read_failures:
        # A file that was not read was not checked, and an unread project file
        # leaves its MRC files unknown. Neither "clean" nor the findings hold.
        print(
            f"arc-ownership-check: {scan.read_failures} file(s) could not be read", file=sys.stderr
        )
        return 2
    if errors > 0 or (strict and notes > 0):
        print(f"arc-ownership-check: {errors} error(s) in files compiled with ARC", file=sys.stderr)
        print(
            "arc-ownership-check: delete the attribute, or list the file as MRC "
            "if it truly needs MRC",
            file=sys.stderr,
        )
        return 1
    print("arc-ownership-check: clean", file=sys.stderr)
    return 0


def main(argv: list[str]) -> int:
    """Scan one repository and return the exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", help="repository root to scan (default: .)")
    parser.add_argument("--strict", action="store_true", help="fail on a note as well as an error")
    parser.add_argument("--verbose", action="store_true", help="list the files treated as MRC")
    arguments = parser.parse_args(argv)

    root = Path(arguments.root).resolve()
    if not root.is_dir():
        print(f"arc-ownership-check: no such directory: {root}", file=sys.stderr)
        return 2

    scan = Scan()
    for project_file in walk(root):
        if project_file.suffix == ".pbxproj":
            add_mrc_files(project_file, scan)
    if arguments.verbose:
        for path in sorted(scan.mrc_paths):
            print(f"arc-ownership-check: MRC file: {path}", file=sys.stderr)
    return report(scan, root, arguments.strict)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
