use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::files::FILES;
use super::{capabilities, files, find, listing_json, manifest};

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Every file under `dir`, hidden files included, as paths relative to `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// The bundle folders under `bundles/`.
fn folders() -> BTreeSet<String> {
    std::fs::read_dir(repo("bundles"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_manifest_names_each_folder_once() {
    let bundles = manifest().unwrap();
    let names: Vec<&str> = bundles.iter().map(|b| b.name.as_str()).collect();
    let unique: BTreeSet<String> = names.iter().map(ToString::to_string).collect();
    assert_eq!(unique.len(), names.len(), "a name is in the manifest twice");
    assert_eq!(unique, folders());
    for bundle in &bundles {
        assert!(
            !bundle.when.is_empty() && !bundle.purpose.is_empty(),
            "{bundle:?}"
        );
    }
}

#[test]
fn the_embedded_table_matches_the_folder() {
    let mut on_disk = BTreeSet::new();
    for bundle in folders() {
        for rel in walk(&repo("bundles").join(&bundle)) {
            on_disk.insert(format!("{bundle}/{}", rel.to_string_lossy()));
        }
    }
    let embedded: BTreeSet<String> = FILES
        .iter()
        .map(|f| format!("{}/{}", f.bundle, f.path))
        .collect();
    assert_eq!(embedded.len(), FILES.len(), "a file is in the table twice");
    assert_eq!(
        embedded, on_disk,
        "add or remove rows in src/onboarding/shipped/files.rs"
    );
    for file in FILES {
        let path = repo("bundles").join(file.bundle).join(file.path);
        assert_eq!(file.text, std::fs::read_to_string(&path).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                file.executable,
                mode & 0o111 != 0,
                "{}: the executable flag in the table differs from the file",
                path.display()
            );
        }
    }
}

/// Each shipped bundle.yaml loads with the loader that reads it after setup.
#[test]
fn each_shipped_bundle_loads() {
    let dir = tempfile::tempdir().unwrap();
    for bundle in manifest().unwrap() {
        let root = dir.path().join(&bundle.name);
        for file in files(&bundle.name) {
            let path = root.join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, file.text).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        crate::merge::read_bundle_yaml(&root, &bundle.name)
            .unwrap_or_else(|e| panic!("{}: {e:#}", bundle.name));
        capabilities(&bundle.name).unwrap();
    }
}

#[test]
fn an_unknown_name_lists_the_shipped_names() {
    let err = format!("{:#}", find("cobol-dev").unwrap_err());
    assert!(
        err.contains("rust-dev") && err.contains("no shipped bundle"),
        "{err}"
    );
    assert_eq!(find("rust-dev").unwrap().when, ["lang-rust"]);
}

#[test]
fn a_bundle_with_comments_only_has_no_capabilities() {
    assert!(capabilities("build").unwrap().is_empty());
    assert!(
        capabilities("kubernetes").unwrap().is_empty(),
        "no bundle.yaml"
    );
    assert!(capabilities("rust-dev").unwrap().contains_key("hooks"));
}

#[test]
fn the_listing_has_each_bundle_with_its_files() {
    let listing: serde_json::Value = serde_json::from_str(&listing_json().unwrap()).unwrap();
    let rust = listing
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "rust-dev")
        .unwrap();
    assert_eq!(rust["when"][0], "lang-rust");
    assert!(
        rust["files"]
            .as_array()
            .unwrap()
            .contains(&"rules/rust.md".into()),
        "{rust}"
    );
    assert!(rust["purpose"].as_str().is_some_and(|p| !p.is_empty()));
}

/// The named shapes of `bundles/personal-patterns.txt`.
fn patterns() -> Vec<(String, Regex)> {
    let text = std::fs::read_to_string(repo("bundles/personal-patterns.txt")).unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (name, pattern) = l.split_once(char::is_whitespace).unwrap();
            let regex = Regex::new(pattern.trim())
                .unwrap_or_else(|e| panic!("pattern {name} does not compile: {e}"));
            (name.to_string(), regex)
        })
        .collect()
}

/// Reserved for documentation by RFC 2606 and RFC 6761.
fn is_reserved_domain(domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    ["example.com", "example.net", "example.org"].contains(&domain.as_str())
        || [".example", ".test", ".invalid"]
            .iter()
            .any(|tld| domain.ends_with(tld))
}

/// The meaning check after a shape match: true when the matched text is a personal value.
fn is_personal(name: &str, matched: &str) -> bool {
    match name {
        "email" => {
            let (local, domain) = matched.split_once('@').unwrap_or_default();
            local != "git" && !is_reserved_domain(domain)
        }
        "ipv4" => matched
            .parse::<Ipv4Addr>()
            .is_ok_and(|ip| ip.is_private() || ip.is_link_local()),
        _ => true,
    }
}

/// Each personal value in `text`, as `line: pattern name: matched text`.
fn personal_hits(text: &str, patterns: &[(String, Regex)]) -> Vec<String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for (name, regex) in patterns {
            for m in regex.find_iter(line) {
                if is_personal(name, m.as_str()) {
                    out.push(format!("{}: {name}: {}", n + 1, m.as_str()));
                }
            }
        }
    }
    out
}

#[test]
fn the_personal_check_finds_personal_values_and_allows_placeholders() {
    let patterns = patterns();
    let personal = [
        "mail me at sam@corp.io",
        "cd /Users/sam/src",
        "cd /home/sam",
        "ssh build.local",
        "ssh box.tail1234.ts.net",
        "addr: 10.1.2.3",
        "addr: 192.168.0.9",
        "addr: 172.20.0.1",
        "addr: 169.254.1.1",
    ];
    for text in personal {
        assert_eq!(personal_hits(text, &patterns).len(), 1, "{text}");
    }
    let allowed = [
        "--apple-id you@example.com",
        "user@host.test",
        "git@github.com:owner/repo.git",
        "version 1.2.3.4 and 8.8.8.8",
        "~/.local/share and @pytest.mark.skipif",
        "addr: 999.1.1.1",
    ];
    for text in allowed {
        assert!(personal_hits(text, &patterns).is_empty(), "{text}");
    }
}

/// A shipped file holds general rules only (Q3 of docs/design/issue-2773-onboarding.md).
#[test]
fn shipped_bundles_hold_no_personal_pattern() {
    let patterns = patterns();
    let mut hits = Vec::new();
    for root in ["bundles", "skills"] {
        let dir = repo(root);
        for rel in walk(&dir) {
            if rel == Path::new("personal-patterns.txt") {
                continue;
            }
            let text = std::fs::read_to_string(dir.join(&rel)).unwrap();
            for hit in personal_hits(&text, &patterns) {
                hits.push(format!("{root}/{}:{hit}", rel.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "shipped files hold personal values. Fix: move each value to your own bundle, or \
         replace it with a placeholder such as you@example.com:\n{}",
        hits.join("\n")
    );
}
