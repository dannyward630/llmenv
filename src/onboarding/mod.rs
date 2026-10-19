//! Fact primitives for the onboarding flow of `llmenv setup` (#2773). Each one returns evidence as
//! JSON for the onboarding skill, and makes no judgment. Design:
//! docs/design/issue-2773-onboarding.md (Decision 2).

pub(crate) mod scan;
pub(crate) mod tools;
