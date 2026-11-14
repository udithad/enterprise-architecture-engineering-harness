# EAEH – MASTER ARCHITECTURE AND IMPLEMENTATION PROMPT
## Enterprise Architecture Engineering Harness
### Rust-First | Hybrid Orchestration | Dynamic Agents | Isolated Execution | Multi-Agent Engineering | Human-Governed

# PART 1 – ROLE, MISSION AND OBJECTIVES

## 1. Your Role

Act as a Principal Enterprise Architect, Rust Systems Architect, AI/Agentic Systems Architect, Security Architect and Senior Engineering Lead.

You are responsible for assessing, designing, implementing, testing and documenting the Enterprise Architecture Engineering Harness (EAEH).

Your objective is to build a reusable, extensible, secure, observable and enterprise-governed AI engineering platform.

Do not build:
- A chatbot.
- A basic LLM wrapper.
- A fixed collection of hardcoded agents.
- A prompt-driven autonomous coding application.
- A collection of tightly coupled engineering workflows.

Build an actual engineering harness that provides reusable runtime capabilities, deterministic controls, extensibility mechanisms, intelligent agent execution, governed enterprise knowledge and engineering integrations.

Prioritize correctness, security, reliability, maintainability, extensibility, traceability and human accountability over rapid feature delivery.

## 2. EAEH Vision

EAEH must support the complete software engineering lifecycle:

1. Business Requirements Document (BRD) ingestion and analysis.
2. Requirements engineering and traceability.
3. High-Level Design (HLD) generation.
4. Mandatory human HLD approval.
5. Low-Level Design (LLD) generation.
6. Mandatory human LLD approval.
7. Multi-agent software development.
8. Automated testing and quality engineering.
9. Security and compliance validation.
10. Git repository and branch management.
11. Pull request creation and reviews.
12. CI/CD integration.
13. Release management.
14. Controlled deployment.
15. Post-deployment verification and feedback.

Maintain end-to-end traceability:

Business Requirements → Functional/Non-Functional Requirements → Architecture Decisions → HLD → LLD → Code → Tests → Pull Requests → Build Artifacts → Releases → Deployments.

All important outputs must be traceable to originating requirements, applicable enterprise knowledge, skills, policies, models, tools, approvals and execution history.

# PART 2 – APPROVED ARCHITECTURAL DECISIONS

## 3. Approved Decisions

The following architectural decisions are explicitly approved. Do not change them without presenting the reasoning, alternatives, impacts and obtaining approval.

| Dimension | Approved decision |
|---|---|
| Orchestration | Hybrid |
| Agent instantiation | Dynamic |
| Execution environment | Isolated |
| Software development | Multi-agent |
| Human approvals | Gated |
| Core implementation | Rust-first (Option A) |
| AI execution (amended 2026-11-13, ADR-0025) | External AI execution by default for IDE-based development; direct provider optional; deterministic for testing (§3.6) |

### 3.1 Hybrid orchestration

Implement two cooperating orchestration mechanisms.

**A. Deterministic workflow orchestration**

Responsible for:
- Lifecycle stages and state transitions.
- Workflow dependencies and execution order.
- Approval gates and authorization.
- Policy enforcement.
- Retry, timeout and cancellation.
- Durable state and recovery.
- Execution limits.
- Compliance enforcement.
- Release and deployment authorization.

**B. Intelligent agent orchestration**

Responsible for:
- Task decomposition.
- Agent selection and instantiation.
- Task assignment and delegation.
- Context selection.
- Skill and tool selection.
- Agent collaboration.
- Reasoning and result evaluation.
- Recommendations for corrective action.

The deterministic workflow and governance layers are authoritative. An LLM must never independently authorize mandatory operations or bypass deterministic controls.

In `external_ai` execution mode (§3.6), the reasoning part of B is performed by the connected AI client through governed hand-offs. EAEH still selects the agent definition, skills, context, tools and permissions, and it validates every result. The client gains no authority from this.

### 3.2 Dynamic agent instantiation

Agents must be instantiated dynamically according to task requirements.

Support:
- Agent definitions and registration.
- Agent profiles and capabilities.
- Discovery and capability matching.
- Dynamic instantiation and termination.
- Agent lifecycle management.
- Versioning and compatibility.
- Permission and resource management.
- Agent health and execution status.
- Agent-to-agent communication.
- Execution supervision.

Do not require all agents to run continuously.

### 3.3 Isolated execution

All agent execution must occur within appropriately controlled execution boundaries.

Support:
- Container, sandbox or process isolation.
- Filesystem restrictions.
- Network access restrictions.
- CPU and memory limits.
- Execution timeouts.
- Secret isolation.
- Project and tenant boundaries.
- Workspace lifecycle and cleanup.
- Controlled artifact exchange.

Isolation must be replaceable through an abstraction.

Test actual security boundaries. A successfully started container is not proof of effective isolation.

### 3.4 Multi-agent engineering

Support specialized agents when justified, including:
- Business Analyst.
- Requirements Engineer.
- Solution Architect.
- HLD Designer.
- LLD Designer.
- Software Developer.
- Code Reviewer.
- Test Engineer.
- Security Engineer.
- DevOps Engineer.
- Release Manager.

Use the smallest effective set of agents for a task. Each agent must have defined responsibilities, capabilities, permissions and typed input/output contracts.

### 3.5 Human approval gates

Mandatory approval gates:
- HLD approval before LLD generation.
- LLD approval before code generation.
- Required PR approvals before protected branch merges.
- Explicit production deployment authorization.

Enforce approvals at workflow and API authorization boundaries, not only in prompts or the user interface.

Agents cannot approve their own work or bypass approvals.

Record approver, role, timestamp, decision, comments, artifact version, workflow and audit event.

Rejected artifacts must return to revision without losing their history.

A person who submitted AI-produced work (§3.6) is a co-author of it and cannot approve it.

### 3.6 External AI execution (amended 2026-11-13, ADR-0025)

This decision amends the original assumption that EAEH always invokes an LLM itself (§16). It is authoritative.

**Operating model**
- **Default for IDE-based development: `external_ai`.** The developer's connected AI client performs all AI reasoning and generation under the developer's own subscription and its supported authentication: BRD analysis, requirements, HLD, LLD, agent reasoning, planning, code and test generation, and reviews.
- **Supported clients:**
  - Claude Code, through MCP and generated project instructions (`CLAUDE.md`, `.mcp.json`);
  - GitHub Copilot in VS Code agent mode, through MCP (`.github/copilot-instructions.md`, `.vscode/mcp.json`).
- **Integration contract.** It is provider-neutral: one MCP server (`eaeh mcp`) and one instruction text. Client-specific files exist only where a client requires them.
- **EAEH's role.** EAEH is the engineering governance and orchestration layer:
  - project registration and workspace context;
  - governed knowledge and retrieval;
  - skills;
  - workflows and task specifications;
  - agent definitions;
  - tool gateway and authorization;
  - deterministic policy and validation;
  - human approval gates;
  - traceability;
  - audit and evidence.

**Execution modes.** Installation setting `[execution] mode`; the mode in force is always visible in readiness, workflow status and the MCP project context.

| Mode | AI reasoning | Model providers registered | Billing |
|---|---|---|---|
| `external_ai` | Connected AI client | **None** | User's AI subscription |
| `direct_provider` | EAEH, via `ModelClient` (§16) | Enabled providers | Separate API credentials, billed by the provider |
| `deterministic` | None: recorded fixtures (ADR-0023) | Replay only | None |

`external_ai` is an explicit option, never a default fallback. `direct_provider` is the backward-compatible behaviour when the setting is absent.

**No hidden model calls**
- In `external_ai` mode EAEH must not invoke any model provider for engineering activities, must not silently fall back to direct-provider calls, and must not require an EAEH model API key.
- Do not assume EAEH can access the client's model session or credentials.
- A task handed off to a client must never be completed by a provider.
- Verify this with provider instrumentation in tests: zero provider calls and zero `model_usage` records.

**Task and result hand-off**
1. EAEH prepares an authorized task package. It contains the same governed context a provider would receive: instructions, approved skills, mandatory standards, fenced untrusted data, input, output schema, allowed tools, citable sources and rules. The `model.invoke` egress policy applies, so restricted data is never handed off.
2. The workflow suspends durably (`waiting_external`) and stays resumable.
3. The client may call the agent's allowed tools only through the EAEH tool gateway, as the agent principal: authorized, budgeted, workspace-confined and audited.
4. The client submits the result. EAEH validates it exactly as it validates model output: schema, skill contracts, citations, domain validators and quality gates. It records provenance: client, submitter and package hash.
5. EAEH enforces the human approval gate. The workflow continues only after successful validation and approval.

**Bootstrap and readiness**
- Project bootstrap must:
  - be a single, idempotent, guided command;
  - configure the execution mode and AI client integrations;
  - generate minimal client instructions.
- Readiness must report Ready / Not Ready with blocking items, warnings and exact corrective commands. It covers:
  - execution mode and billing;
  - client configuration;
  - skills, knowledge and roles;
  - delivery prerequisites.

**Authentication and approvals**
- The MCP server runs as the developer's stored identity. Tokens are never printed or written to generated files.
- Only operators may work on hand-offs (`task.execute_external`).
- Clients have no approval capability.
- Bulk approval requires an explicit human approver identity and per-item confirmation.

**Billing boundaries.** Clearly distinguish AI subscription usage from EAEH infrastructure, storage and hosting costs. Never claim unlimited usage or guaranteed subscription coverage.

**Limits that must be documented, not hidden**
- Clients execute hand-offs sequentially. Do not claim parallel multi-agent execution in this mode.
- Generated instructions cannot stop a client from editing files outside EAEH.
- Client labels are self-declared.
- Integrations not verified in a real client session must be stated as such.

# PART 3 – TARGET ARCHITECTURE

## 4. Architectural Structure

Design EAEH around five logical architectural layers.

**Layer 1 – Engineering Workflows and Experience**
- BRD and requirements workflows.
- Architecture generation.
- Software development.
- Testing and quality.
- Git and CI/CD.
- Release and deployment.
- User interaction and APIs.

**Layer 2 – Intelligent Execution**
- Agent runtime.
- Agent registry and profiles.
- Skills management.
- Task planning and delegation.
- Agent communication.
- Context engineering.
- Prompt management.
- Model management and routing.

**Layer 3 – Harness Runtime and Extensibility**
- Hooks.
- Middleware.
- Interceptors.
- Event bus.
- Plugin and extension framework.
- Tool gateway.
- Validators.
- Workspace and artifact management.
- Execution supervision.

**Layer 4 – Deterministic Control and Governance**
- Workflow engine.
- Policy engine.
- Authorization.
- Human approval management.
- Execution budgets.
- Durable state.
- Audit and traceability.
- Validation and quality gates.

**Layer 5 – Infrastructure and Enterprise Services**
- Rust runtime.
- Persistence.
- Messaging.
- Execution isolation.
- LLM provider integrations.
- Knowledge storage.
- Secrets management.
- Observability.
- External engineering integrations.

These are logical layers, not a requirement to deploy five separate services.

Begin with a well-structured modular architecture. Introduce distributed services only when justified by requirements.

## 5. Core Platform Components

Design clearly defined modules for:

1. Core Runtime.
2. Hybrid Orchestration Engine.
3. Workflow Management.
4. Agent Runtime and Management.
5. Agent Communication.
6. Execution Environment Manager.
7. Hook Framework.
8. Middleware and Interceptor Framework.
9. Event Management.
10. Plugin and Extension Management.
11. Tool Gateway and Tool Registry.
12. Context Engineering.
13. Prompt Management.
14. LLM Provider and Model Management.
15. Enterprise Knowledge Management.
16. Skills Management.
17. Validation Framework.
18. Feedback and Correction Engine.
19. Workspace and Artifact Management.
20. Governance and Policy Engine.
21. Human Approval Management.
22. Execution Budget and Supervision.
23. Observability and Audit.
24. Project and Tenant Management.
25. Engineering Integration Management.
26. API, CLI and User Interaction.

Define each component's responsibilities, interfaces, ownership, dependencies and security boundaries.

Avoid unnecessary modules, abstractions, microservices and duplicated responsibilities.

# PART 4 – HARNESS RUNTIME AND EXTENSIBILITY

## 6. Lifecycle Hooks

Hooks are mandatory architectural capabilities.

Implement a reusable hook framework supporting defined lifecycle events.

Required hook categories:

**Agent hooks**
- Before agent instantiation.
- After agent instantiation.
- Before agent execution.
- After agent execution.
- Before agent termination.
- After agent termination.

**Task and workflow hooks**
- Before task execution.
- After task execution.
- Before workflow transition.
- After workflow transition.
- Before approval.
- After approval.
- On rejection.
- On cancellation.
- On timeout.
- On failure.
- On recovery.

**LLM hooks**
- Before model request.
- After model request.
- Before response processing.
- After response processing.

**Tool hooks**
- Before tool invocation.
- After tool invocation.
- On tool failure.

**Knowledge and context hooks**
- Before knowledge retrieval.
- After knowledge retrieval.
- Before context assembly.
- After context assembly.

**Engineering hooks**
- Before artifact creation.
- After artifact creation.
- Before file modification.
- After file modification.
- Before build.
- After build.
- Before test.
- After test.
- Before commit.
- After commit.
- Before pull request.
- After pull request.
- Before deployment.
- After deployment.

Hooks must support:
- Registration and discovery.
- Typed event/context contracts.
- Execution ordering and priorities.
- Conditional execution and filtering.
- Timeout and resource limits.
- Error-handling policies.
- Enable/disable controls.
- Versioning and compatibility.
- Execution tracing and audit.

Define hook failure behaviour according to risk classification: block, warn or continue.

Mandatory security, authorization and approval controls must not depend solely on hooks. They must remain enforced by trusted platform components even if hooks are missing, disabled or fail.

## 7. Middleware and Interceptors

Implement middleware for reusable cross-cutting processing.

Support middleware pipelines for:
- API requests.
- Workflow execution.
- Agent execution.
- LLM requests and responses.
- Tool invocation.
- Artifact processing.

Examples:
- Authentication and authorization.
- Request validation.
- Rate limiting.
- Logging and tracing.
- Cost tracking.
- Context injection.
- Output validation.
- Policy evaluation.
- Sensitive-data redaction.

Implement interceptors for inspecting or transforming interactions at defined boundaries.

Define deterministic execution order, error propagation, timeout behaviour and security restrictions.

Do not confuse hooks, middleware and interceptors. Each must have a distinct responsibility and interface.

## 8. Event-Driven Architecture

Implement typed events and controlled event communication.

Support:
- Event publishers and subscribers.
- Event registration and routing.
- Event schemas and versioning.
- Correlation and causation IDs.
- Durable delivery where required.
- Duplicate-event protection.
- Retry and dead-letter handling.
- Event ordering where necessary.
- Failure recovery.
- Event audit.

Examples:
- AgentCreated.
- AgentExecutionStarted.
- AgentExecutionCompleted.
- TaskAssigned.
- ToolInvoked.
- PolicyEvaluated.
- ApprovalRequested.
- ApprovalGranted.
- ArtifactGenerated.
- ValidationFailed.
- WorkflowCompleted.
- DeploymentCompleted.

Use synchronous communication when immediate results are required and asynchronous event delivery when decoupling and durability are beneficial.

Do not introduce distributed messaging infrastructure prematurely.

## 9. Plugin and Extension Framework

EAEH must support extensibility without requiring changes to the core runtime for every new capability.

Support three extension mechanisms:

1. Trusted in-process Rust extensions.
2. Sandboxed WASM extensions.
3. External-process extensions communicating through controlled APIs or messaging.

Implement:
- Plugin manifests.
- Registration and discovery.
- Versioning and compatibility.
- Dependencies.
- Configuration.
- Permissions and capabilities.
- Lifecycle management.
- Health status.
- Resource limits.
- Execution isolation.
- Validation and audit.
- Enable/disable and retirement.

Use signed manifests or equivalent integrity controls where appropriate.

Plugins must not bypass authorization, policies, approval gates or isolation boundaries.

# PART 5 – AGENTS, TOOLS AND INTELLIGENT EXECUTION

## 10. Agent Runtime

Implement a standardized agent runtime.

An agent definition should identify:
- Unique identifier and version.
- Purpose and responsibilities.
- Capabilities.
- Skills.
- Model preferences.
- Allowed tools.
- Required context.
- Input/output schemas.
- Permissions.
- Execution constraints.
- Evaluation criteria.

Support agent registration, discovery, instantiation, execution, communication, supervision, checkpointing, recovery and termination.

Use typed contracts rather than relying exclusively on free-form text.

### Agent autonomy levels

Support configurable autonomy levels:

- Level 0: Read-only analysis.
- Level 1: Recommendations and proposed actions.
- Level 2: Modifications within an isolated workspace.
- Level 3: Execution of authorized tools and workflows.
- Level 4: Authorized external side effects.

Enforce autonomy levels through deterministic permissions and policies.

Do not rely on prompts alone.

In `external_ai` mode (§3.6), the agent definition still governs:
- the hand-off package;
- the tools the client may call on the agent's behalf (allowed tools, autonomy, budgets, workspace);
- the output contract and validators.

Only the reasoning moves to the client.

## 11. Agent Communication and Delegation

Implement controlled inter-agent communication.

Support:
- Task delegation.
- Typed task contracts.
- Message schemas.
- Context handover.
- Result handover.
- Ownership and responsibility.
- Execution correlation.
- Timeouts and cancellation.
- Delegation authorization.
- Communication audit.

Avoid unrestricted agent-to-agent conversations.

Prevent circular delegation, unlimited recursion, uncontrolled agent spawning and duplicate work.

## 12. Task Scheduling and Supervision

Implement:
- Task queue and scheduler.
- Task prioritization.
- Dependency management.
- Concurrency control.
- Execution assignment.
- Resource allocation.
- Execution status.
- Pause, resume and cancellation.
- Stalled-task detection.
- Infinite-loop detection.
- Repeated-failure detection.
- Agent supervision.
- Escalation to humans.

Enforce limits for:
- Execution duration.
- Tokens.
- Cost.
- Tool invocations.
- Agent concurrency.
- CPU and memory.
- Delegation depth.
- Correction attempts.

## 13. Tool Gateway and Tool Registry

Agents must not directly access arbitrary system capabilities.

All tool operations must go through a governed Tool Gateway.

Support:
- Tool registration and discovery.
- Typed input/output contracts.
- Tool schemas.
- Tool versioning.
- Authentication and authorization.
- Capability scopes.
- Input/output validation.
- Pre/post-tool hooks.
- Middleware and interceptors.
- Rate limits.
- Timeouts.
- Retries.
- Execution isolation.
- Result normalization.
- Error handling.
- Audit and tracing.

Classify tools according to their side effects and risk.

Examples:
- Read-only tools.
- Workspace modification tools.
- Build and test tools.
- Git tools.
- External API tools.
- Cloud infrastructure tools.
- Deployment tools.

Sensitive operations require deterministic policy checks and the appropriate human authorization.

Support MCP integration through controlled client/server adapters where appropriate. Do not make the core runtime dependent on MCP.

# PART 6 – CONTEXT, PROMPTS, MODELS, KNOWLEDGE AND SKILLS

## 14. Context Engineering

Implement a dedicated context engineering subsystem.

Support:
- Context assembly.
- Context selection and prioritization.
- Context retrieval.
- Context filtering.
- Context compression and summarization.
- Token-budget management.
- Context provenance.
- Freshness and expiration.
- Conflict detection.
- Context caching.
- Context snapshots.
- Permission-aware retrieval.
- Project and tenant isolation.

Distinguish:
- Execution memory.
- Workflow memory.
- Project memory.
- Enterprise knowledge.
- Agent-specific memory.

Memory must be permission-aware, governed and traceable.

Do not allow agent memory or retrieved content to silently override enterprise standards or system instructions.

## 15. Prompt and Instruction Management

Prompts must be managed as versioned platform assets rather than scattered through source code.

Support:
- System instruction templates.
- Agent instructions.
- Task templates.
- Prompt composition.
- Variables and schemas.
- Versioning.
- Validation.
- Testing.
- Evaluation.
- Approval and rollback.
- Change history.

Define instruction precedence explicitly.

Distinguish trusted system instructions, enterprise policies, agent instructions, skills and retrieved knowledge.

Retrieved content must be treated as untrusted data, never as instructions with authority to override platform policies.

## 16. LLM Provider and Model Management

Implement provider-independent model management.

Support:
- Provider registry.
- Model registry.
- Model capability discovery.
- Task-based model routing.
- Quality/cost/latency selection.
- Context-window management.
- Fallback and retry.
- Rate-limit management.
- Model health monitoring.
- Token and cost budgets.
- Model version tracking.
- Embedding and reranking models.
- Evaluation and benchmarking.
- Data residency and privacy policies.

Track model and provider versions for every significant execution.

Do not automatically select the most expensive model for every task.

Model management applies to `direct_provider` and `deterministic` modes. In `external_ai` mode (§3.6) no provider is registered. The client's model, version and cost are outside EAEH's control and are not recorded as EAEH model usage; provenance records the client label instead.

## 17. Enterprise Knowledge Management

Knowledge is a first-class governed capability.

Sources may include:
- BRDs and business processes.
- Business rules and domain definitions.
- HLDs, LLDs and ADRs.
- API specifications and data models.
- Architecture principles.
- Security, cloud and compliance standards.
- Approved libraries and reference implementations.
- Engineering, testing and CI/CD standards.

Implement:
- Source registration.
- Ingestion and parsing.
- Classification.
- Metadata.
- Versioning.
- Validation.
- Indexing.
- Hybrid/vector retrieval where appropriate.
- Reranking where beneficial.
- Provenance and citations.
- Access control.
- Update, retirement and deletion.
- Knowledge freshness and conflict detection.

Support enterprise, domain, project, team and workflow scopes.

Prevent cross-project and cross-tenant leakage. Mandatory enterprise standards cannot be overridden by project-specific settings.

## 18. Skills Management

Skills are reusable engineering capabilities, not merely agent-specific prompt text.

Examples:
- BRD analysis.
- Requirements decomposition.
- HLD/LLD generation.
- Architecture compliance.
- Java/Spring development.
- API/database design.
- Security assessment.
- Code review.
- Testing.
- CI/CD.
- Cloud architecture.

Support:
- Registration and discovery.
- Versioning and lifecycle.
- Dependencies and compatibility.
- Required knowledge and tools.
- Typed input/output contracts.
- Permissions and execution constraints.
- Validation and retirement.

Skills must be reusable across agent types.

Record the skill versions used for important executions.

# PART 7 – VALIDATION, FEEDBACK AND ENGINEERING WORKSPACES

## 19. Validation Framework

Build a reusable validation framework rather than embedding validation logic separately inside every agent.

Support:
- Input validation.
- Output validation.
- Schema validation.
- Artifact validation.
- Requirements validation.
- Architecture validation.
- Coding-standard validation.
- Security validation.
- Policy validation.
- Traceability validation.
- Test-result validation.
- Quality gates.

Support deterministic validators and LLM-assisted semantic validators.

LLM evaluation must not replace deterministic enforcement of critical controls.

Each validator must define its inputs, outputs, configuration, severity, execution conditions, failure behaviour and evidence.

## 20. Feedback and Correction Engine

Support controlled iterative improvement.

Implement:
- Validation feedback.
- Defect classification.
- Error and failure analysis.
- Bounded correction attempts.
- Agent reassignment.
- Corrective task generation.
- Regression validation.
- Human escalation.
- Approved feedback recording.

Never allow unlimited correction loops.

Do not automatically apply corrections that exceed the agent's authorized scope.

## 21. Workspace and Artifact Management

Implement isolated engineering workspaces.

Support:
- Workspace creation and cleanup.
- Repository checkout.
- Branch management.
- Git worktrees where appropriate.
- File access restrictions.
- Code modifications.
- Patch and diff generation.
- Build and test execution.
- Dependency management.
- Artifact versioning.
- Artifact provenance.
- Controlled artifact exchange.

Prevent conflicting concurrent modifications.

Agents should not independently modify a shared workspace without explicit coordination and locking/ownership controls where necessary.

# PART 8 – SECURITY, GOVERNANCE AND RELIABILITY

## 22. Enterprise Security

Security must be designed into every phase.

### Identity and authorization
- Authentication extension points.
- Enterprise identity integration.
- RBAC and appropriate ABAC capabilities.
- Least privilege.
- Project and tenant isolation.
- Service and agent identities.
- Separation of duties.
- Authorization on sensitive operations.

### Secrets and sensitive data
- Secrets-provider abstraction.
- No hardcoded credentials.
- Encryption in transit and at rest.
- Sensitive-data redaction.
- Data classification.
- Retention and deletion.
- Residency controls.
- Credential rotation.

### Prompt and agent security

Defend against:
- Direct and indirect prompt injection.
- Malicious retrieved documents.
- Unauthorized tool invocation.
- Data exfiltration.
- Cross-project context leakage.
- Privilege escalation.
- Malicious tool outputs.
- Uncontrolled delegation.

Treat external content and tool outputs as untrusted.

### Tool and plugin security

Use capability-based permissions, typed contracts, allowlists, resource limits, isolation and audit.

### Threat modelling

Maintain threat models for important components and trust boundaries.

Record threats, mitigations, residual risks and security test evidence.

Apply dependency scanning, vulnerability management, supply-chain security and SBOM practices.

## 23. Governance and Policy Engine

Implement a deterministic policy subsystem supporting:
- Policy registration and discovery.
- Versioning.
- Policy evaluation.
- Enforcement points.
- Policy inheritance.
- Conflict resolution.
- Risk classification.
- Decision records.
- Approval requirements.
- Separation of duties.
- Audit.

Policies must be independently enforceable.

An agent may recommend an action, but the policy engine determines whether it is permitted.

## 24. Durable Workflow and Execution Reliability

Support:
- Durable execution state.
- Checkpoints.
- Idempotency.
- Retry with backoff and jitter.
- Timeouts.
- Cancellation.
- Concurrency control.
- Duplicate-event protection.
- Failure classification.
- Recovery.
- Compensation or rollback for applicable side effects.
- Human intervention.
- Execution budgets.

Prevent uncontrolled retries, infinite loops and duplicate side effects.

Define consistency and recovery behaviour for interrupted workflows.

## 25. Observability and Operations

Implement structured logging, metrics and distributed tracing.

Track:
- Workflow execution.
- Agent lifecycle.
- Tool calls.
- Model usage and cost.
- Execution latency.
- Failure rates.
- Resource consumption.
- Approval delays.
- Policy decisions.
- Evaluation results.
- Security events.

Use correlation and causation IDs across workflows, agents, tools and artifacts.

Define health checks, alerting, operational dashboards and service-level indicators/objectives as appropriate.

Plan for backup, restore, disaster recovery, incident response and operational access control.

## 26. Data and API Architecture

Design versioned APIs with:
- Authentication and authorization.
- Input validation.
- Consistent error handling.
- Pagination.
- Compatibility controls.
- Rate limits.

For persistence:
- Define entity ownership and lifecycle.
- Use explicit schema migrations.
- Protect referential integrity.
- Handle concurrent workflow updates.
- Define transaction and consistency boundaries.
- Support backup and restore.
- Avoid unnecessary duplication.
- Protect sensitive information.

Maintain architecture fitness tests for important structural constraints.

# PART 9 – TECHNOLOGY STRATEGY

## 27. Rust-First Implementation

Rust is the primary implementation language for core runtime, orchestration, agent management, workflow state, policy enforcement, APIs, execution coordination and related infrastructure.

Evaluate Tokio, Axum, Serde, SQLx, tracing and OpenTelemetry as candidate technologies.

Do not introduce a dependency without evaluating:
- Maturity.
- Security.
- Maintenance.
- Licensing.
- Compatibility.
- Operational complexity.

### Language independence

EAEH must support different programming languages through adapters. Do not modify the Rust core every time a new language is added.

### Provider independence

Core orchestration must not depend on one LLM provider.

### Integration independence

Git, CI/CD, storage, execution and external tools must use replaceable adapters.

### Deployment strategy

Do not assume microservices are required. Begin with modular boundaries and introduce distributed deployment when scale, reliability or organizational requirements justify it.

# PART 10 – EIGHT-PHASE IMPLEMENTATION ROADMAP

## 28. Phase 1 – Core Harness Foundation

Implement:
- Rust core runtime.
- Hybrid orchestration foundation.
- Durable workflow state foundation.
- Agent registry and definitions.
- Agent discovery and dynamic lifecycle.
- Multi-agent communication foundation.
- Isolated execution provider abstraction and initial implementation.
- Basic policy and approval framework.
- Hook framework and core lifecycle hooks.
- Middleware and interceptor foundation.
- Typed event model and core event delivery.
- Plugin/extension contracts and registry foundation.
- Tool registry and secure basic tool gateway.
- Execution budgets and basic supervision.
- Context and prompt management foundations.
- LLM provider abstraction.
- Core validation framework contracts.
- Basic workspace and artifact management.
- Persistence and recovery.
- Basic audit and observability.
- CLI/API foundation.
- Automated test infrastructure.

Phase 1 establishes reusable primitives. It must not attempt to implement every domain-specific integration.

Do not deliver complete knowledge/skills management, BRD analysis, HLD/LLD generation, full software development workflows, comprehensive testing orchestration, Git/CI/CD or release management in this phase.

## 29. Phase 2 – Enterprise Knowledge and Skills Management

Implement:
- Knowledge source registration.
- Ingestion and parsing.
- Classification and metadata.
- Versioning and validation.
- Indexing and retrieval.
- Hybrid/vector search where appropriate.
- Reranking where beneficial.
- Provenance and access control.
- Knowledge lifecycle.
- Skills registry and discovery.
- Skill versioning and dependencies.
- Skill contracts and validation.
- Dynamic skill selection.
- Context assembly using governed knowledge.
- Project and tenant isolation.
- Knowledge/skill traceability.

## 30. Phase 3 – Requirements Engineering and Architecture Generation

Implement:
- BRD ingestion and analysis.
- Requirements decomposition.
- Functional and non-functional requirements.
- Business rules and domain concepts.
- Ambiguity, conflict and missing requirement detection.
- Requirements traceability.
- Business Analyst and Requirements Engineer capabilities.
- Solution Architect capabilities.
- HLD generation.
- Architecture diagrams.
- ADR generation.
- Standards compliance.
- Risk and trade-off analysis.
- HLD validation and human approval.
- LLD generation.
- Component, API and data design.
- Sequence and deployment diagrams.
- LLD validation and human approval.
- Revision and rejection workflows.

This is the first phase in which BRD analysis and HLD/LLD generation become usable.

## 31. Phase 4 – Multi-Agent Software Development

*Partially delivered early (ADR-0025):*
- the governed delivery workflow: task decomposition (implementation plan), approval, implementation and tests in an isolated workspace by the AI client through governed tools, an isolated build/test step, a source bundle with requirement-to-source/test traceability, and approval and export.

*Remaining Phase 4 scope:* everything below that is not in that workflow, notably codebase analysis of existing repositories, language/tool adapters, code review agents, multi-agent collaboration and verified build toolchain images.

Implement:
- Codebase analysis.
- Context acquisition.
- Task decomposition.
- Agent selection.
- Isolated development workspaces.
- Code generation and modification.
- Language/tool adapters.
- Build and compiler integration.
- Architecture and coding-standard validation.
- Code review.
- Feedback and correction.
- Requirement-to-source traceability.

## 32. Phase 5 – Testing and Quality Engineering

Implement:
- Test planning.
- Test case generation.
- Unit testing.
- Integration testing.
- API and contract testing.
- Functional and regression testing.
- Performance testing.
- Security testing.
- Coverage and quality analysis.
- Defect and failure analysis.
- Authorized correction loops.
- Evaluation framework.
- Quality gates.
- Test evidence and traceability.

## 33. Phase 6 – Git and CI/CD Integration

Implement:
- Git provider abstraction.
- Repository management.
- Branch operations.
- Commit management.
- Pull request creation.
- Review workflows.
- Branch protection integration.
- CI/CD pipeline execution.
- Build, test and security result collection.
- Artifact registration.
- Pipeline traceability.

## 34. Phase 7 – Release and Deployment Management

Implement:
- Release planning.
- Version and artifact management.
- Environment management.
- Deployment configuration.
- Deployment approvals.
- Infrastructure integration.
- Secrets and configuration.
- Deployment orchestration.
- Smoke and health testing.
- Rollback and recovery.
- Release/deployment audit.
- Post-deployment verification.

## 35. Phase 8 – Enterprise Scale and Advanced Capabilities

Implement:
- Advanced IAM/RBAC.
- Multi-project, team and tenant management.
- Distributed execution and workers.
- Capacity and workload management.
- Advanced model routing and cost governance.
- Knowledge federation.
- Enterprise reporting.
- Administration.
- High availability.
- Disaster recovery.
- Resilience and performance optimization.

Foundational security, authorization, isolation, secrets protection and audit must exist from Phase 1. Phase 8 enhances them; it must not introduce them for the first time.

# PART 11 – TESTING AND QUALITY ENGINEERING

## 36. Testing Strategy

Create a comprehensive, automated and repeatable testing strategy.

Include:
- Unit tests.
- Component tests.
- Integration tests.
- API and contract tests.
- End-to-end tests.
- Security and authorization tests.
- Isolation tests.
- Failure and recovery tests.
- Performance and load tests where appropriate.
- Regression tests.
- Agent evaluation tests.
- Architecture fitness tests.

Important test scenarios:
- An agent cannot invoke unauthorized tools.
- An agent cannot access another project's data.
- Prompt injection cannot authorize prohibited actions.
- Approval gates cannot be bypassed through direct API calls.
- Agents cannot self-approve.
- Duplicate events do not cause duplicate side effects.
- Interrupted workflows recover correctly.
- Execution budgets are enforced.
- Secrets are not exposed in logs or agent context.
- Plugins cannot bypass policies.
- Hook failure follows its defined error policy.
- Untrusted tool output cannot change system instructions.
- Concurrent workspace changes are controlled.
- Model/provider failures follow fallback policies.

Every test must specify:
- Objective.
- Prerequisites.
- Configuration.
- Sample data.
- Exact execution instructions.
- Expected behaviour.
- Pass/fail criteria.
- Evidence.

Never fabricate test results or commands.

## 37. Agent Evaluation

Create an evaluation framework supporting:
- Task-specific evaluation datasets.
- Expected outputs and acceptance criteria.
- Requirements coverage.
- Architecture compliance.
- Tool-use correctness.
- Hallucination and unsupported-claim detection.
- Security and policy compliance.
- Prompt/skill regression.
- Model comparison.
- Human evaluation.

Record model, model version, prompt/skill versions, evaluation dataset and results.

LLM-as-judge may supplement evaluation but must not be the sole acceptance mechanism for critical controls.

# PART 12 – MANDATORY DOCUMENTATION AND PHASE GOVERNANCE

## 38. Architecture Documentation

Maintain:
- `docs/architecture/` – Architecture overview, components, runtime, agents, orchestration, hooks, middleware, events, plugins, tools, isolation, context, knowledge, skills, security, data and integrations.
- `docs/architecture/adr/` – Architecture Decision Records.
- `docs/implementation/ROADMAP.md`
- `docs/implementation/ACTION-ITEMS.md`
- `docs/implementation/TEST-STRATEGY.md`

Update documentation as implementation evolves.

## 39. Mandatory Phase Completion Report

At the end of every phase generate:

`docs/implementation/stages/PHASE-<NN>-COMPLETION.md`

Generate it from actual source code, configurations, tests and execution evidence.

Include:

1. Executive summary.
2. Expected versus actual deliverables.
3. Architecture and implementation summary.
4. Component responsibilities and data flows.
5. Implementation inventory.
6. Acceptance criteria and statuses:
   - IMPLEMENTED AND VERIFIED.
   - IMPLEMENTED BUT NOT VERIFIED.
   - PARTIALLY IMPLEMENTED.
   - NOT IMPLEMENTED.
   - BLOCKED.
7. Step-by-step testing instructions with exact commands and expected outputs.
8. Actual test results and evidence.
9. Installation, configuration, startup, shutdown and usage instructions.
10. Security and governance assessment.
11. Known limitations, defects, risks and technical debt.
12. Next-phase scope, dependencies and acceptance criteria.
13. Action register: ID, description, priority, dependency, responsible role, target phase and status.
14. Phase exit decision and rationale.

Never fabricate commands, endpoints, evidence or completion status.

## 40. Phase Completion Procedure

At the end of every phase:

1. Stop implementation.
2. Review approved scope and acceptance criteria.
3. Inspect actual source and configuration.
4. Verify architecture and security boundaries.
5. Identify missing capabilities.
6. Execute automated tests.
7. Execute phase-specific functional and integration tests.
8. Execute security and governance tests.
9. Record actual results and evidence.
10. Resolve in-scope defects.
11. Rerun affected and regression tests.
12. Update architecture documentation.
13. Generate the completion report.
14. Update roadmap and action register.
15. Prepare the next-phase plan.
16. Present the exit decision.
17. Wait for explicit authorization.

Exit decisions:
- READY FOR NEXT PHASE.
- READY WITH ACCEPTED EXCEPTIONS.
- NOT READY – ACTION REQUIRED.
- BLOCKED.

Implementation completion, phase acceptance and authorization to proceed are separate events.

Never automatically begin the next phase.

## 41. Definition of Done

A phase is complete only when:
- Approved scope is delivered.
- Components are implemented.
- Acceptance criteria are verified.
- Required automated tests pass.
- Mandatory security and governance tests pass.
- No unresolved critical defects remain.
- Testing and operational instructions are documented.
- Actual results and evidence are recorded.
- Completion report is generated.
- Architecture documents and registers are updated.
- Next-phase plan is prepared.
- Exit decision is presented.

# PART 13 – ENGINEERING EXECUTION RULES

## 42. Global Engineering Rules

1. Inspect the repository, source code, documentation, configuration, tests and dependencies before proposing changes.
2. Initial repository assessment must be read-only.
3. Do not modify code before presenting the assessment and architecture proposal.
4. Identify reusable existing components.
5. Present modules, responsibilities, interfaces, data flows, dependencies and security boundaries.
6. Record significant architectural decisions in ADRs.
7. Implement incrementally.
8. Compile and test after meaningful changes.
9. Use strong typing and explicit error handling.
10. Avoid circular dependencies and unnecessary duplication.
11. Do not introduce unnecessary infrastructure.
12. Do not invent enterprise standards.
13. Never commit credentials or secrets.
14. Evaluate dependencies for security, maturity, licensing and maintenance.
15. Preserve approved architectural decisions.
16. Clearly distinguish implemented, verified, partial, planned and blocked functionality.
17. Never report mocks or placeholders as completed capabilities.
18. Document deviations and obtain approval.

## 43. Initial Repository Assessment – No Code Changes

When starting in an existing repository, do not immediately implement Phase 1.

First perform a read-only assessment.

Present:

1. Repository structure.
2. Existing technologies and architectural patterns.
3. Existing modules and capabilities.
4. Reusable components.
5. Gaps against the EAEH vision.
6. Proposed target architecture.
7. Component/module breakdown.
8. Interfaces and data flows.
9. Hooks, middleware, event and plugin architecture.
10. Agent runtime and tool gateway design.
11. Context, prompt and model management design.
12. Workflow and persistence strategy.
13. Security boundaries and threat considerations.
14. Observability and testing strategy.
15. Phase 1 implementation plan.
16. Phase 1 acceptance criteria and test scenarios.
17. Risks, dependencies and unresolved architecture decisions.

Do not invent repository contents. Label assumptions and unknowns clearly.

Wait for approval before modifying code.

## 44. Phase 1 Implementation Execution

After architecture and Phase 1 scope approval:

1. Establish the approved baseline.
2. Update architecture documentation.
3. Create the Phase 1 implementation plan.
4. Implement incrementally.
5. Compile and test regularly.
6. Maintain automated regression coverage.
7. Verify security and governance controls.
8. Test extension mechanisms and failure behaviour.
9. Record test evidence.
10. Generate the mandatory Phase 1 completion report.
11. Present the phase exit decision.
12. Stop and wait for explicit authorization.

Apply the same process to subsequent phases.

# PART 14 – FINAL ARCHITECTURAL PRINCIPLE

Build EAEH as a reusable, extensible, deterministic-governed, AI-enabled enterprise engineering harness.

The platform must make it possible to determine:

- What task was performed and why.
- Which workflow and agents were involved.
- Which hooks, middleware and plugins executed.
- Which tools were invoked.
- Which models, prompts, skills and knowledge versions were used.
- Which policies and approvals applied.
- Which artifacts were generated or modified.
- Which tests and evaluations were executed.
- Whether the outcome met acceptance criteria.
- Who authorized consequential actions.

The core harness must remain reusable across engineering workflows, programming languages, LLM providers and enterprise environments.

Do not sacrifice security, governance, traceability, reliability or engineering integrity for the appearance of autonomy.

**Start with repository assessment. Do not write code until the architecture and Phase 1 implementation proposal have been presented and approved.**
