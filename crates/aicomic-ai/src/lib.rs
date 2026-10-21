//! Native AI workflows and legacy-compatible file storage.
//! Constructors never contact a provider. `root` is the application data root.
mod client;
mod error;
mod prompts;
mod settings;
mod store;
mod workflow;

pub use client::{ChatMessage, LlmClient, extract_json};
pub use error::{AiError, Result};
pub use prompts::{PromptRegistry, PromptTemplate};
pub use settings::AiSettings;
pub use store::{ContextStore, StageMeta};
pub use workflow::{
    ExecutionInput, ReviewDecision, WorkflowEngine, WorkflowEvent, WorkflowResult,
    reusable_artifacts,
};
