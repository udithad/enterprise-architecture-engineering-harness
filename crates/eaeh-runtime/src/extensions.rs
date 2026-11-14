//! Host for trusted in-process Rust extensions (D-03, clarification 1).
//!
//! Extensions are compiled into the binary and handed to the [`crate::HarnessBuilder`].
//! Nothing is loaded dynamically. On startup each extension's manifest is registered
//! (integrity-verified); the extension contributes components **only if** an
//! administrator has enabled that exact manifest version, and only through the
//! capabilities granted. The registration context exposes registries, never the database,
//! authorizer, policy engine or approval manager, and every contributed tool remains
//! subject to the tool gateway.

use eaeh_core::{HarnessError, PluginId, Result};
use eaeh_extensibility::{Hook, HookRegistration, HookRegistry, PluginCapability, PluginManifest};
use eaeh_intelligence::{ModelProvider, ModelRegistry};
use eaeh_tools::{Tool, ToolRegistry};
use std::collections::BTreeSet;
use std::sync::Arc;

pub trait InProcessExtension: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn register(&self, ctx: &mut ExtensionContext<'_>) -> Result<()>;
}

pub struct ExtensionContext<'a> {
    pub(crate) plugin: PluginId,
    pub(crate) granted: BTreeSet<PluginCapability>,
    pub(crate) hooks: &'a HookRegistry,
    pub(crate) tools: &'a ToolRegistry,
    pub(crate) models: &'a ModelRegistry,
}

impl ExtensionContext<'_> {
    fn require(&self, c: PluginCapability) -> Result<()> {
        if self.granted.contains(&c) {
            Ok(())
        } else {
            Err(HarnessError::forbidden(format!(
                "plugin {} was not granted {c:?}",
                self.plugin
            )))
        }
    }

    pub fn add_hook(&mut self, mut reg: HookRegistration, hook: Arc<dyn Hook>) -> Result<()> {
        self.require(PluginCapability::RegisterHooks)?;
        reg.source = self.plugin.to_string();
        reg.id = format!("{}/{}", self.plugin, reg.id);
        self.hooks.register(reg, hook)
    }

    pub fn add_tool(&mut self, tool: Arc<dyn Tool>) -> Result<()> {
        self.require(PluginCapability::RegisterTools)?;
        self.tools.register(tool, self.plugin.as_str())
    }

    pub fn add_model_provider(&mut self, p: Arc<dyn ModelProvider>) -> Result<()> {
        self.require(PluginCapability::RegisterModelProviders)?;
        self.models.register(p)
    }
}
