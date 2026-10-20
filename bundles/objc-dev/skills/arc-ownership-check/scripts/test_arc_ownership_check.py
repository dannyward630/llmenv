"""Tests for arc_ownership_check.py. The project file goes through plutil, so macOS only."""

import os
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).with_name("arc_ownership_check.py")
pytestmark = pytest.mark.skipif(shutil.which("plutil") is None, reason="needs macOS plutil")


def _project(root: Path) -> None:
    """Two Foo.m files: MRC/Foo.m builds with -fno-objc-arc, ARC/Foo.m does not."""
    objects = {
        "P": {"isa": "PBXProject", "mainGroup": "G0"},
        "G0": {"isa": "PBXGroup", "children": ["G1", "G2"], "sourceTree": "<group>"},
        "G1": {"isa": "PBXGroup", "path": "MRC", "children": ["F1"], "sourceTree": "<group>"},
        "G2": {"isa": "PBXGroup", "path": "ARC", "children": ["F2"], "sourceTree": "<group>"},
        "F1": {"isa": "PBXFileReference", "path": "Foo.m", "sourceTree": "<group>"},
        "F2": {"isa": "PBXFileReference", "path": "Foo.m", "sourceTree": "<group>"},
        "B1": {
            "isa": "PBXBuildFile",
            "fileRef": "F1",
            "settings": {"COMPILER_FLAGS": "-fno-objc-arc"},
        },
        "B2": {"isa": "PBXBuildFile", "fileRef": "F2"},
    }
    (root / "App.xcodeproj").mkdir()
    (root / "App.xcodeproj" / "project.pbxproj").write_bytes(
        plistlib.dumps({"objects": objects, "rootObject": "P"})
    )
    for folder in ("MRC", "ARC"):
        (root / folder).mkdir()
        (root / folder / "Foo.m").write_text("- (void)show __attribute__((ns_consumes_self));\n")


def _run(root: Path, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), "--root", str(root)],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )


def test_mrc_file_hides_only_itself_not_an_arc_file_of_the_same_name(tmp_path):
    _project(tmp_path)
    done = _run(tmp_path)
    assert done.returncode == 1, done.stderr
    assert f"{tmp_path / 'ARC' / 'Foo.m'}:1:ns_consumes_self" in done.stdout
    assert "MRC" not in done.stdout


def test_clean_project_passes(tmp_path):
    _project(tmp_path)
    (tmp_path / "ARC" / "Foo.m").write_text("- (void)show;\n")
    done = _run(tmp_path)
    assert done.returncode == 0, done.stderr
    assert "clean" in done.stderr


@pytest.mark.skipif(os.geteuid() == 0, reason="root reads any file")
def test_unreadable_source_is_not_clean(tmp_path):
    _project(tmp_path)
    (tmp_path / "ARC" / "Foo.m").write_text("- (void)show;\n")
    locked = tmp_path / "ARC" / "Bar.m"
    locked.write_text("x\n")
    locked.chmod(0)
    try:
        done = _run(tmp_path)
    finally:
        locked.chmod(0o644)
    assert done.returncode == 2
    assert "could not be read" in done.stderr


def test_missing_plutil_is_not_clean(tmp_path):
    _project(tmp_path)
    (tmp_path / "ARC" / "Foo.m").write_text("- (void)show;\n")
    done = _run(tmp_path, env={"PATH": str(tmp_path / "no-bin")})
    assert done.returncode == 2
    assert "plutil not found" in done.stderr
