//! Typed HLD and LLD models. Architects (agents) draft them as JSON; the platform parses
//! strictly, validates, renders and gates them.

use serde::{Deserialize, Serialize};

pub const HLD_KIND: &str = "hld";
pub const LLD_KIND: &str = "lld";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Person,
    ExternalSystem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    /// Lower-case slug, unique among actors and components.
    pub id: String,
    pub name: String,
    pub kind: ActorKind,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemContext {
    pub system_name: String,
    pub description: String,
    #[serde(default)]
    pub actors: Vec<Actor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Service,
    WebApp,
    MobileApp,
    Gateway,
    Batch,
    Database,
    Queue,
    Cache,
    Storage,
    Library,
    External,
}

impl ComponentKind {
    /// Components that must be designed in the LLD and deployed.
    pub fn is_buildable(self) -> bool {
        matches!(
            self,
            ComponentKind::Service
                | ComponentKind::WebApp
                | ComponentKind::MobileApp
                | ComponentKind::Gateway
                | ComponentKind::Batch
        )
    }
    pub fn is_deployable(self) -> bool {
        !matches!(self, ComponentKind::Library | ComponentKind::External)
    }
    pub fn label(self) -> &'static str {
        match self {
            ComponentKind::Service => "service",
            ComponentKind::WebApp => "web app",
            ComponentKind::MobileApp => "mobile app",
            ComponentKind::Gateway => "gateway",
            ComponentKind::Batch => "batch",
            ComponentKind::Database => "database",
            ComponentKind::Queue => "queue",
            ComponentKind::Cache => "cache",
            ComponentKind::Storage => "storage",
            ComponentKind::Library => "library",
            ComponentKind::External => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// Lower-case slug, unique among actors and components.
    pub id: String,
    pub name: String,
    pub kind: ComponentKind,
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub technology: String,
    /// Requirement ids allocated to this component.
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub id: String,
    /// Component offering the interface.
    pub provider: String,
    /// Components or actors using it.
    pub consumers: Vec<String>,
    pub protocol: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlow {
    pub id: String,
    pub from: String,
    pub to: String,
    pub data: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentNode {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub technology: String,
    /// Component ids hosted on this node.
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub option: String,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
}

/// Architecture decision; rendered as an ADR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// `D-nnn` (HLD) or `LD-nnn` (LLD).
    pub id: String,
    pub title: String,
    pub status: DecisionStatus,
    pub context: String,
    pub decision: String,
    pub alternatives: Vec<Alternative>,
    #[serde(default)]
    pub consequences: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<String>,
    /// Standard document citations (`src/subject@vN`) this decision relates to.
    #[serde(default)]
    pub standards: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    /// `R-nnn`.
    pub id: String,
    pub description: String,
    pub likelihood: Level,
    pub impact: Level,
    pub mitigation: String,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeOff {
    /// `T-nnn`.
    pub id: String,
    pub concern: String,
    pub options: Vec<String>,
    pub choice: String,
    pub rationale: String,
    #[serde(default)]
    pub quality_attributes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceStatus {
    Compliant,
    Deviation,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComplianceEntry {
    /// Standard document citation `src/subject@vN`.
    pub standard: String,
    pub status: ComplianceStatus,
    /// Evidence (compliant) or justification (deviation / not applicable).
    pub justification: String,
    /// Decision id documenting a deviation.
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hld {
    /// Always `"hld"`.
    pub document: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub context: SystemContext,
    pub components: Vec<Component>,
    #[serde(default)]
    pub interfaces: Vec<Interface>,
    #[serde(default)]
    pub data_flows: Vec<DataFlow>,
    #[serde(default)]
    pub deployment: Vec<DeploymentNode>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub risks: Vec<Risk>,
    #[serde(default)]
    pub tradeoffs: Vec<TradeOff>,
    #[serde(default)]
    pub compliance: Vec<ComplianceEntry>,
    #[serde(default)]
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub name: String,
    pub responsibility: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDesign {
    /// HLD component id this design refines.
    pub component: String,
    #[serde(default)]
    pub description: String,
    pub modules: Vec<Module>,
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiOperation {
    pub id: String,
    pub component: String,
    /// `GET|POST|PUT|PATCH|DELETE` (HTTP, path starts with `/`) or `PUBLISH|SUBSCRIBE` (topic).
    pub method: String,
    pub path: String,
    pub summary: String,
    /// Entity names of the request / response payloads.
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub response: Option<String>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    pub target: String,
    pub cardinality: Cardinality,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub name: String,
    /// Owning component id.
    pub owner: String,
    #[serde(default)]
    pub description: String,
    pub attributes: Vec<Attribute>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceStep {
    pub from: String,
    pub to: String,
    pub message: String,
    /// API operation id invoked by this step (its component must be `to`).
    #[serde(default)]
    pub operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    /// `S-nnn`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub steps: Vec<SequenceStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lld {
    /// Always `"lld"`.
    pub document: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub components: Vec<ComponentDesign>,
    #[serde(default)]
    pub apis: Vec<ApiOperation>,
    #[serde(default)]
    pub data: Vec<Entity>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    #[serde(default)]
    pub deployment: Vec<DeploymentNode>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub risks: Vec<Risk>,
    #[serde(default)]
    pub compliance: Vec<ComplianceEntry>,
    #[serde(default)]
    pub citations: Vec<String>,
}
