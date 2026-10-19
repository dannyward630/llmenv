//! The servers that `managed_servers` resolves for the health probe.

use super::*;

#[test]
fn an_unrelated_mcp_entry_does_not_hide_the_memory_server() {
    // A public-IP `http://` entry fails resolution (#2483), and it must not stop the
    // health check from seeing the memory server.
    let config: crate::config::Config = serde_yaml::from_str(
        r#"
host:
  still: { addr: "10.0.0.4" }
features:
  memory:
    - server_host: still
      port: 7878
      when: [t]
mcp:
  - name: ctx7
    when: [t]
    type: http
    url: http://93.184.216.34/mcp
"#,
    )
    .unwrap();
    let active = crate::scope::ActiveScopes {
        tags: ["t".to_string()].into(),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let servers = managed_servers(&config, dir.path(), &active).unwrap();
    assert!(
        servers.iter().any(|s| s.name == MEMORY_MCP_NAME),
        "{servers:?}"
    );
}

#[test]
fn a_codebase_memory_server_that_only_a_bundle_declares_is_probed() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("bundles").join("cbm");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bundle.yaml"),
        "features:\n  codebase_memory:\n    - when: [t]\n",
    )
    .unwrap();
    let config = crate::config::Config {
        bundle: vec![crate::config::Bundle {
            name: "cbm".to_string(),
            when: vec!["t".to_string()],
        }],
        ..crate::config::Config::default()
    };
    let active = crate::scope::ActiveScopes {
        scopes: vec![crate::scope::ActiveScope {
            id: "host".to_string(),
            kind: "host",
            tags: vec!["t".to_string()],
            ..crate::scope::ActiveScope::default()
        }],
        tags: ["t".to_string()].into(),
        ..Default::default()
    };
    let servers = managed_servers(&config, root.path(), &active).unwrap();
    assert!(
        servers.iter().any(|s| s.name == CODEBASE_MEMORY_MCP_NAME),
        "{servers:?}"
    );
}
