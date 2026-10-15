//! Notices for the agent, carried by hook output. The list lives in `llmenv_util`, so the
//! task crate and the hook code share one list.

pub(crate) use llmenv_util::agent_notice::{current, record};
