//! Property tests for reading a plugin manifest. The manifest comes from a plugin folder, so any
//! bytes may appear in it, and a malformed file must give an error, never a panic.

use super::*;
use proptest::prelude::*;

#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
fn write_bytes(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("manifest.json");
    std::fs::write(&path, bytes).expect("write manifest fixture");
    (dir, path)
}

proptest! {
    /// Arbitrary bytes in a hooks manifest return Ok or Err, and never panic.
    #[test]
    fn parse_plugin_hooks_never_panics_on_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let (dir, path) = write_bytes(&bytes);
        let _ = parse_plugin_hooks(&path, dir.path(), "plugin");
    }

    /// Arbitrary bytes in an MCP config return Ok or Err, and never panic.
    #[test]
    fn parse_plugin_mcp_entries_never_panics_on_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let (_dir, path) = write_bytes(&bytes);
        let _ = parse_plugin_mcp_entries(&path, "plugin");
    }
}
