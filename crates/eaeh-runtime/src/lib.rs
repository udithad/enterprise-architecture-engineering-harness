//! EAEH runtime: composition root, configuration, executors, supervision,
//! observability and the trusted in-process extension host.

pub mod config;
pub mod coordinator;
pub mod executor;
pub mod extensions;
pub mod harness;
pub mod maintenance;
pub mod observability;
pub mod onboarding;
pub mod project;
pub mod supervision;

pub use config::HarnessConfig;
pub use eaeh_agents::external::{DeliverySummary, INTERACTIVE};
pub use eaeh_agents::{ExecutionMode, ExternalSubmission, ExternalTaskView};
pub use eaeh_workspace::{
    ArtifactMeta, INDEX_FILE, MaterializationStatus, MaterializeReport, WorkspaceRegistration,
};
pub use extensions::{ExtensionContext, InProcessExtension};
pub use harness::{Harness, HarnessBuilder, HealthReport};
