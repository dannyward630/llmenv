//! The expansion of a leading `~` to `$HOME`.

use std::env::VarError;

/// Expand a leading `~` or `~/` to `$HOME`. Other input is returned unchanged.
///
/// # Errors
/// Returns an error that names `HOME` when the input starts with `~` and `HOME` is unset, empty,
/// or not valid UTF-8. Such a path cannot be expanded, and it must not be used as is: a relative
/// `~/x` would otherwise resolve against the working directory.
pub fn expand_tilde(p: &str) -> anyhow::Result<String> {
    expand_tilde_with_env(p, &|name| std::env::var(name))
}

/// [`expand_tilde`] with an injectable env-var provider so tests can exercise
/// the set-but-empty and non-UTF-8 `HOME` cases without mutating real process env vars.
pub(crate) fn expand_tilde_with_env(
    p: &str,
    get_env: &impl Fn(&str) -> Result<String, VarError>,
) -> anyhow::Result<String> {
    // `rest` is empty for `~` and starts with `/` for `~/x`. `~user` is not a home reference.
    let rest = match p.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
        _ => return Ok(p.to_string()),
    };
    // HOME is read only for a `~` input, so an absolute or relative path never fails on it.
    let home = match get_env("HOME") {
        // An empty HOME would anchor "~/rest" at the filesystem root ("/rest"). An unset
        // HOME would leave "~/rest" relative to the working directory (#1179, #2679).
        Ok(home) if !home.is_empty() => home,
        Ok(_) | Err(VarError::NotPresent) => anyhow::bail!(
            "cannot expand {p:?}: HOME is not set or is empty. Set HOME, or give an absolute path"
        ),
        Err(VarError::NotUnicode(_)) => anyhow::bail!(
            "cannot expand {p:?}: HOME is not valid UTF-8. Set HOME to a UTF-8 path and retry"
        ),
    };
    Ok(format!("{home}{rest}"))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn home(value: Option<&str>) -> impl Fn(&str) -> Result<String, VarError> + '_ {
        move |_| value.map(str::to_string).ok_or(VarError::NotPresent)
    }

    proptest! {
        /// Only `~` and `~/rest` expand. Anything else comes back as it was, whatever HOME is.
        #[test]
        fn only_a_bare_tilde_or_a_tilde_slash_expands(
            path in "[~a-z/.]{0,12}",
            value in proptest::option::of("/[a-z]{1,8}"),
        ) {
            let expands = path == "~" || path.starts_with("~/");
            let got = expand_tilde_with_env(&path, &home(value.as_deref()));
            match (expands, value.as_deref()) {
                (false, _) => prop_assert_eq!(got.unwrap(), path),
                (true, Some(h)) => prop_assert_eq!(got.unwrap(), format!("{h}{}", &path[1..])),
                (true, None) => prop_assert!(got.unwrap_err().to_string().contains("HOME")),
            }
        }
    }

    #[test]
    fn an_empty_home_is_an_error() {
        let err = expand_tilde_with_env("~/x", &home(Some(""))).unwrap_err();
        assert!(err.to_string().contains("HOME"), "{err}");
    }
}
