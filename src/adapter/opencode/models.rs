//! Model-provider and default-model rendering for `opencode.json` (#2847).
//!
//! The parent module owns the serialized types. This module owns how llmenv maps its
//! provider and model settings onto them.

use super::*;

/// Map llmenv's `ModelProvider.api_type` (an open wire-format string) to the
/// AI SDK npm package opencode's `provider.<id>.npm` field expects.
///
/// Known short forms expand to the common case; anything else (including a
/// literal `@ai-sdk/...` package name) passes through unchanged, since a
/// caller who already knows the exact package should be able to name it
/// directly rather than being forced through a fixed enum.
fn opencode_provider_npm(api_type: Option<&str>) -> String {
    match api_type {
        Some("anthropic") => "@ai-sdk/anthropic".to_string(),
        Some("google") => "@ai-sdk/google".to_string(),
        Some(other) if other != "openai" => other.to_string(),
        // No documented default for opencode; openai-compatible covers the
        // overwhelming majority of self-hosted/proxy providers this feature
        // targets (Ollama, vLLM, LM Studio, most OpenAI-compatible proxies).
        Some(_) | None => "@ai-sdk/openai-compatible".to_string(),
    }
}

/// Render `capabilities.model_providers` into opencode's `provider.<id>` map.
pub(super) fn render_opencode_providers(
    providers: &[llmenv_config::ModelProvider],
) -> BTreeMap<String, OpencodeProviderEntry> {
    providers
        .iter()
        .filter(|p| !p.disabled)
        .map(|p| {
            let models = (!p.models.is_empty()).then(|| {
                p.models
                    .iter()
                    .map(|m| (m.id.clone(), render_opencode_model(m)))
                    .collect()
            });
            let entry = OpencodeProviderEntry {
                npm: opencode_provider_npm(p.api_type.as_deref()),
                name: p.name.clone(),
                options: OpencodeProviderOptions {
                    base_url: p.base_url.clone(),
                    api_key: p.api_key.clone(),
                    headers: (!p.headers.is_empty()).then(|| p.headers.clone()),
                },
                models,
            };
            (p.id.clone(), entry)
        })
        .collect()
}

fn render_opencode_model(m: &llmenv_config::ModelSource) -> OpencodeModelEntry {
    OpencodeModelEntry {
        name: m.name.clone(),
        // opencode's schema requires both `context` and `output` when `limit`
        // is present at all — only emit it when llmenv has both.
        limit: match (m.context_window, m.max_tokens) {
            (Some(context), Some(output)) => Some(OpencodeModelLimit { context, output }),
            _ => None,
        },
        cost: m.cost.as_ref().map(|c| OpencodeModelCost {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
        }),
        modalities: (!m.modalities.is_empty()).then(|| OpencodeModalities {
            input: m.modalities.clone(),
        }),
    }
}

/// Render `default_models` into opencode's `model`/`small_model` top-level
/// fields, as `provider_id/model_id` strings. opencode has exactly two
/// default-model slots — `large` maps to `model`, `small` maps to
/// `small_model` — so any other role name has no destination and is dropped.
pub(super) fn render_opencode_default_models(
    caps: &llmenv_config::Capabilities,
) -> (Option<String>, Option<String>) {
    let render = |r#ref: &llmenv_config::ModelRef| format!("{}/{}", r#ref.provider, r#ref.model);
    let large = caps
        .default_models
        .get("large")
        .map(|r#ref| with_effort_variant(render(r#ref), &r#ref.model, caps));
    (large, caps.default_models.get("small").map(render))
}

/// Append the effort as an opencode model variant, `provider/model#variant` (#2847).
///
/// The model's own effort wins over the global `effort_level`. The variant name is the
/// effort value as written. The variant is not checked against opencode here.
fn with_effort_variant(
    model: String,
    model_id: &str,
    caps: &llmenv_config::Capabilities,
) -> String {
    let effort = caps
        .model_effort
        .get(model_id)
        .and_then(|e| e.effort_level.as_deref())
        .or(caps.effort_level.as_deref());
    match effort {
        Some(level) => format!("{model}#{level}"),
        None => model,
    }
}

#[cfg(test)]
mod tests {
    use super::render_opencode_default_models;
    use llmenv_config::{Capabilities, ModelEffort, ModelRef};

    fn caps_with_models() -> Capabilities {
        let mut caps = Capabilities::default();
        caps.default_models.insert(
            "large".into(),
            ModelRef {
                provider: "anthropic".into(),
                model: "big".into(),
            },
        );
        caps.default_models.insert(
            "small".into(),
            ModelRef {
                provider: "anthropic".into(),
                model: "tiny".into(),
            },
        );
        caps
    }

    #[test]
    fn global_effort_becomes_the_large_model_variant() {
        let mut caps = caps_with_models();
        caps.effort_level = Some("high".into());
        let (large, _) = render_opencode_default_models(&caps);
        assert_eq!(large.as_deref(), Some("anthropic/big#high"));
    }

    #[test]
    fn per_model_effort_overrides_the_global_effort() {
        let mut caps = caps_with_models();
        caps.effort_level = Some("high".into());
        caps.model_effort.insert(
            "big".into(),
            ModelEffort {
                effort_level: Some("low".into()),
                max_effort_level: None,
            },
        );
        let (large, _) = render_opencode_default_models(&caps);
        assert_eq!(large.as_deref(), Some("anthropic/big#low"));
    }

    #[test]
    fn no_effort_leaves_the_model_id_plain() {
        let (large, small) = render_opencode_default_models(&caps_with_models());
        assert_eq!(large.as_deref(), Some("anthropic/big"));
        assert_eq!(small.as_deref(), Some("anthropic/tiny"));
    }

    #[test]
    fn small_model_never_takes_the_effort_variant() {
        let mut caps = caps_with_models();
        caps.effort_level = Some("high".into());
        let (_, small) = render_opencode_default_models(&caps);
        assert_eq!(small.as_deref(), Some("anthropic/tiny"));
    }
}

#[cfg(test)]
mod prop_tests {
    use super::render_opencode_default_models;
    use llmenv_config::{Capabilities, ModelEffort, ModelRef};

    fn large_caps(model: &str) -> Capabilities {
        let mut caps = Capabilities::default();
        caps.default_models.insert(
            "large".into(),
            ModelRef {
                provider: "p".into(),
                model: model.into(),
            },
        );
        caps
    }

    proptest::proptest! {
        /// The variant is the global effort for any model id, and a per-model effort wins.
        #[test]
        fn variant_follows_the_global_then_the_per_model_effort(
            model in "[a-z0-9.-]{1,12}",
            global in "[a-z]{1,8}",
            own in "[a-z]{1,8}",
        ) {
            let mut caps = large_caps(&model);
            caps.effort_level = Some(global.clone());
            let (plain, _) = render_opencode_default_models(&caps);
            proptest::prop_assert_eq!(plain, Some(format!("p/{model}#{global}")));

            caps.model_effort.insert(
                model.clone(),
                ModelEffort { effort_level: Some(own.clone()), max_effort_level: None },
            );
            let (overridden, _) = render_opencode_default_models(&caps);
            proptest::prop_assert_eq!(overridden, Some(format!("p/{model}#{own}")));
        }
    }
}
