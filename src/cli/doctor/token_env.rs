//! The value of a token-efficiency env var that Claude Code sees (#2643).

/// Effective value of a token-efficiency env var: the process environment
/// wins if set (matches what Claude Code will actually see if it inherited
/// the shell), otherwise fall back to `native.claude_code.env` in the
/// resolved config — a var declared there lands in settings.json's own `env`
/// block, which Claude Code applies to itself independent of the shell that
/// launched it, so it counts as "set" even when the shell never exported it.
pub(super) fn effective_token_efficiency_var(
    native_claude_env: Option<&serde_yaml::Value>,
    key: &str,
) -> Option<String> {
    effective_from_read(crate::env_var::utf8_var(key), native_claude_env, key)
}

/// The rule behind [`effective_token_efficiency_var`], split out so a test can supply the read.
/// A value that is not UTF-8 gives no value. It does not fall back to the settings value,
/// because that value may not be the one the process holds (#2643).
fn effective_from_read(
    read: anyhow::Result<Option<String>>,
    native_claude_env: Option<&serde_yaml::Value>,
    key: &str,
) -> Option<String> {
    match read {
        Ok(Some(val)) => Some(val),
        Ok(None) => {
            let value = native_claude_env?.get(key)?;
            value
                .as_str()
                .map(String::from)
                .or_else(|| value.as_bool().map(|b| b.to_string()))
                .or_else(|| value.as_i64().map(|n| n.to_string()))
        }
        Err(e) => {
            tracing::warn!(
                "cannot read {key}: {e:#}. Fix: set {key} to a valid UTF-8 value, then run again."
            );
            None
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn native_env(yaml: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml).expect("valid yaml")
    }

    #[test]
    fn a_read_error_is_logged_with_the_variable_and_the_fix() {
        let read = Err(anyhow::anyhow!("not valid UTF-8"));
        let mut got = None;

        let logs = crate::test_log_capture::capture_logs(|| {
            got = Some(effective_from_read(read, None, "BASH_MAX_OUTPUT_LENGTH"));
        });

        assert_eq!(got, Some(None));
        assert!(
            logs.contains("cannot read BASH_MAX_OUTPUT_LENGTH"),
            "{logs}"
        );
        assert!(logs.contains("Fix: set BASH_MAX_OUTPUT_LENGTH"), "{logs}");
    }

    #[test]
    fn non_utf8_token_var_gives_no_value_and_no_settings_fallback() {
        // The settings file sets the key, so a fallback would report a value.
        let native = native_env("BASH_MAX_OUTPUT_LENGTH: '1000'");
        let read = Err(anyhow::anyhow!("BASH_MAX_OUTPUT_LENGTH is not valid UTF-8"));

        let got = effective_from_read(read, Some(&native), "BASH_MAX_OUTPUT_LENGTH");

        assert_eq!(
            got, None,
            "a non-UTF-8 value must not fall back to settings.json"
        );
    }

    #[test]
    fn unset_token_var_falls_back_to_settings() {
        let native = native_env("BASH_MAX_OUTPUT_LENGTH: 1000");

        let got = effective_from_read(Ok(None), Some(&native), "BASH_MAX_OUTPUT_LENGTH");

        assert_eq!(got.as_deref(), Some("1000"));
    }

    #[test]
    fn set_token_var_wins_over_settings() {
        let native = native_env("BASH_MAX_OUTPUT_LENGTH: 1000");

        let got = effective_from_read(
            Ok(Some("2000".to_string())),
            Some(&native),
            "BASH_MAX_OUTPUT_LENGTH",
        );

        assert_eq!(got.as_deref(), Some("2000"));
    }

    proptest! {
        #[test]
        fn a_read_error_never_falls_back_to_settings(file in "[a-z0-9]{1,10}") {
            let native = native_env(&format!("K: '{file}'"));
            let got = effective_from_read(Err(anyhow::anyhow!("not utf8")), Some(&native), "K");
            prop_assert_eq!(got, None);
        }

        #[test]
        fn a_set_value_always_wins_over_settings(env in "[a-z0-9]{1,10}", file in "[a-z0-9]{1,10}") {
            let native = native_env(&format!("K: '{file}'"));
            let got = effective_from_read(Ok(Some(env.clone())), Some(&native), "K");
            prop_assert_eq!(got, Some(env));
        }

        #[test]
        fn an_unset_value_reads_the_settings_string(file in "[a-z0-9]{1,10}") {
            let native = native_env(&format!("K: '{file}'"));
            let got = effective_from_read(Ok(None), Some(&native), "K");
            prop_assert_eq!(got, Some(file));
        }
    }
}
