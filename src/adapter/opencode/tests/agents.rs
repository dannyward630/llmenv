//! Where materialize writes agents and commands for opencode 2.x (#2844).

use super::*;

#[test]
fn materialize_bundle_agent_from_files() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = MergedManifest::default();
    let src_dir = tempfile::tempdir().unwrap();
    let agent_file = src_dir.path().join("test-agent.md");
    std::fs::write(
        &agent_file,
        "---\ndescription: A test agent\n---\n\nDo the thing.",
    )
    .unwrap();
    manifest
        .files
        .insert(PathBuf::from("agents/test-agent.md"), agent_file);
    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();
    // opencode 2.x reads agents from `agents/` only; `command/` holds slash commands.
    let out_agent = tmp.path().join("agents/test-agent.md");
    assert!(!tmp.path().join("command").read_dir().unwrap().any(|_| true));
    let content = std::fs::read_to_string(&out_agent).unwrap();
    assert!(content.contains("mode: subagent"));
}

#[test]
fn materialize_plugin_commands_and_agents_translated() {
    let out = tempfile::tempdir().unwrap();
    let plugin_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugin_dir.path().join("commands")).unwrap();
    std::fs::write(
        plugin_dir.path().join("commands/greet.md"),
        "---\ndescription: greet\n---\nhello\n",
    )
    .unwrap();
    std::fs::create_dir_all(plugin_dir.path().join("agents")).unwrap();
    std::fs::write(
        plugin_dir.path().join("agents/helper.md"),
        "---\ndescription: helps\n---\nhelp text\n",
    )
    .unwrap();
    let (plugins, marketplaces) = plugin_manifest(plugin_dir.path());
    let manifest = MergedManifest {
        plugins,
        marketplaces,
        ..Default::default()
    };
    OpencodeAdapter.materialize(&manifest, out.path()).unwrap();
    assert!(out.path().join("command/__plugin_greet.md").exists());
    assert!(out.path().join("agents/helper.md").exists());
    assert!(!out.path().join("command/__plugin_agent_helper.md").exists());
}

#[test]
fn materialize_bundle_command_from_files() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = MergedManifest::default();

    let src_dir = tempfile::tempdir().unwrap();
    let cmd_file = src_dir.path().join("test-cmd.md");
    std::fs::write(
        &cmd_file,
        "---\ndescription: A test command\nmodel: claude-sonnet-4-20250514\n---\n\nRun this thing.",
    )
    .unwrap();

    manifest
        .files
        .insert(PathBuf::from("commands/test-cmd.md"), cmd_file);

    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();

    // The translation replaces the bundle copy, so opencode lists the command one time.
    let out_cmd = tmp.path().join("commands/test-cmd.md");
    assert!(!tmp.path().join("command/__bundle_test-cmd.md").exists());
    let content = std::fs::read_to_string(&out_cmd).unwrap();
    assert!(content.contains("description: A test command"));
    assert!(!content.contains("model:")); // dropped for commands
    assert!(content.contains("Run this thing."));
}

#[test]
fn a_bundle_agent_replaces_the_plugin_agent_of_the_same_name() {
    let out = tempfile::tempdir().unwrap();
    let plugin_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugin_dir.path().join("agents")).unwrap();
    std::fs::write(
        plugin_dir.path().join("agents/helper.md"),
        "---\ndescription: from the plugin\n---\nplugin text\n",
    )
    .unwrap();
    let src_dir = tempfile::tempdir().unwrap();
    let agent_file = src_dir.path().join("helper.md");
    std::fs::write(
        &agent_file,
        "---\ndescription: from the bundle\n---\nbundle text\n",
    )
    .unwrap();
    let (plugins, marketplaces) = plugin_manifest(plugin_dir.path());
    let mut manifest = MergedManifest {
        plugins,
        marketplaces,
        ..Default::default()
    };
    manifest
        .files
        .insert(PathBuf::from("agents/helper.md"), agent_file);
    let owned = OpencodeAdapter.materialize(&manifest, out.path()).unwrap();
    let content = std::fs::read_to_string(out.path().join("agents/helper.md")).unwrap();
    assert!(content.contains("bundle text"), "{content}");
    assert!(owned.contains(&PathBuf::from("agents/helper.md")));
}
