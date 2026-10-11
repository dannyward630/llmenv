"""Tests for scripts/roll-changelog.sh.

The script is a cargo-release pre-release hook. cargo-release sets DRY_RUN=true
for hooks during a dry run, and a dry run must not edit the changelog (#2637).
"""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "roll-changelog.sh"

CHANGELOG = """# Changelog

## [Unreleased] - ReleaseDate

### Fixed

- A fix.

<!-- 3.0 next-header -->

## [3.0.0] - 2026-01-01

### Added

- A feature.

<!-- next-url -->
[Unreleased]: https://github.com/example/repo/compare/v3.0.0...HEAD
[3.0.0]: https://github.com/example/repo/releases/tag/v3.0.0
"""


class RollChangelogTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "scripts").mkdir()
        shutil.copy(SCRIPT, self.root / "scripts" / "roll-changelog.sh")
        self.changelog = self.root / "CHANGELOG-3.md"
        self.changelog.write_text(CHANGELOG, encoding="utf-8")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _run(self, dry_run: str | None) -> subprocess.CompletedProcess[str]:
        env = {k: v for k, v in os.environ.items() if k != "DRY_RUN"}
        if dry_run is not None:
            env["DRY_RUN"] = dry_run
        return subprocess.run(
            ["bash", str(self.root / "scripts" / "roll-changelog.sh"), "3.1.0"],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )

    def test_dry_run_leaves_changelog_unchanged(self) -> None:
        result = self._run("true")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.changelog.read_text(encoding="utf-8"), CHANGELOG)

    def test_real_run_rolls_the_unreleased_section(self) -> None:
        result = self._run(None)

        self.assertEqual(result.returncode, 0, result.stderr)
        rolled = self.changelog.read_text(encoding="utf-8")
        self.assertIn("## [3.1.0] - ", rolled)
        self.assertIn("## [Unreleased] - ReleaseDate", rolled)


if __name__ == "__main__":
    unittest.main()
