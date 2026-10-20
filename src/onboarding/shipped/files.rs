//! The embedded files of each shipped bundle. A test checks this table against the `bundles/`
//! folder, so a file added there without a row here fails the build's tests.

/// One file of a shipped bundle.
#[derive(Debug)]
pub(crate) struct Embedded {
    /// The bundle folder name.
    pub(crate) bundle: &'static str,
    /// The path in the bundle folder, with `/` separators.
    pub(crate) path: &'static str,
    pub(crate) text: &'static str,
    /// The file has the executable bit, because a skill runs it directly.
    pub(crate) executable: bool,
}

macro_rules! embed {
    ($bundle:literal, $path:literal) => {
        embed!($bundle, $path, false)
    };
    ($bundle:literal, $path:literal, $executable:literal) => {
        Embedded {
            bundle: $bundle,
            path: $path,
            text: include_str!(concat!("../../../bundles/", $bundle, "/", $path)),
            executable: $executable,
        }
    };
}

pub(crate) const FILES: &[Embedded] = &[
    embed!("build", "bundle.yaml"),
    embed!("build", "rules/cmake.md"),
    embed!("docker", "bundle.yaml"),
    embed!("docker", "rules/docker.md"),
    embed!("go-dev", "bundle.yaml"),
    embed!("go-dev", "rules/go.md"),
    embed!("kubernetes", "rules/crd-design-rust.md"),
    embed!("kubernetes", "rules/operator-rust.md"),
    embed!("kubernetes", "rules/workloads.md"),
    embed!("lang-c-cpp", "rules/c.md"),
    embed!("lang-c-cpp", "rules/cpp.md"),
    embed!("lang-c-cpp", "skills/cpp-concurrency/SKILL.md"),
    embed!("lang-powershell", "bundle.yaml"),
    embed!("lang-powershell", "rules/powershell-hardening.md"),
    embed!("lang-powershell", "rules/powershell.md"),
    embed!("mac-dev", "bundle.yaml"),
    embed!("mac-dev", "rules/bundle-layout.md"),
    embed!("mac-dev", "rules/linking-relocatable.md"),
    embed!("mac-dev", "rules/macos.md"),
    embed!("mac-dev", "rules/signing-notarization.md"),
    embed!("mac-dev", "rules/universal-binaries.md"),
    embed!("objc-dev", "rules/objc.md"),
    embed!(
        "objc-dev",
        "skills/arc-ownership-check/scripts/arc_ownership_check.py"
    ),
    embed!(
        "objc-dev",
        "skills/arc-ownership-check/scripts/check-arc-ownership.sh",
        true
    ),
    embed!(
        "objc-dev",
        "skills/arc-ownership-check/scripts/test_arc_ownership_check.py"
    ),
    embed!("objc-dev", "skills/arc-ownership-check/SKILL.md"),
    embed!("python-dev", "bundle.yaml"),
    embed!("python-dev", "hooks/ruff-report.sh"),
    embed!("python-dev", "hooks/test-ruff-report.sh"),
    embed!("python-dev", "rules/python.md"),
    embed!("rust-dev", "bundle.yaml"),
    embed!("rust-dev", "rules/rust-api.md"),
    embed!("rust-dev", "rules/rust-hardening.md"),
    embed!("rust-dev", "rules/rust.md"),
    embed!("web-dev", "bundle.yaml"),
    embed!("web-dev", "rules/accessibility.md"),
    embed!("web-dev", "rules/e2e-playwright.md"),
    embed!("web-dev", "rules/html-baseline.md"),
    embed!("web-dev", "rules/htmx-accessibility.md"),
    embed!("web-dev", "rules/htmx.md"),
    embed!("web-dev", "rules/typescript.md"),
    embed!("web-dev", "rules/ux-patterns.md"),
    embed!("web-dev", "rules/visual-design.md"),
    embed!("web-dev", "rules/web-stance.md"),
];
