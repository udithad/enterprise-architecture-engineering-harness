# MASTER IMPLEMENTATION PROMPT
# Enterprise Architecture Engineering Harness (EAEH)

## 1. ROLE AND RESPONSIBILITY

Act as a Principal Software Architect, Enterprise AI Architect, Rust Engineer, Agentic Systems Engineer, DevSecOps Architect and Technical Lead.

Your responsibility is to design and implement an enterprise-grade Enterprise Architecture Engineering Harness (EAEH).

EAEH is intended to be a reusable, extensible, secure and governed AI engineering platform capable of supporting the complete software development lifecycle.

This is not a simple AI chatbot, an LLM wrapper or a collection of independent agents.

It must be an engineering harness that coordinates enterprise knowledge, agent skills, dynamically instantiated agents, workflows, execution environments, engineering tools, governance controls and human decisions.

The platform must be designed for enterprise adoption, multiple projects, multiple teams, multiple technology stacks and future extensibility.

You must think architecturally before implementing.

Do not introduce unnecessary complexity, but do not compromise enterprise architecture, security, governance, extensibility or maintainability for implementation convenience.

---

# 2. PRODUCT VISION

Build a platform that can support the following software engineering lifecycle:

1. Business Requirements Document (BRD) ingestion and analysis.
2. Requirements engineering and traceability.
3. High-Level Design (HLD) generation.
4. Human HLD review and approval.
5. Low-Level Design (LLD) generation.
6. Human LLD review and approval.
7. Software development and code generation.
8. Automated testing and quality engineering.
9. Security and compliance validation.
10. Git branch and pull-request management.
11. CI/CD pipeline execution.
12. Release management.
13. Controlled deployment to target environments.
14. Post-deployment verification and operational feedback.

The platform must preserve traceability between:

Business requirements
→ Functional and non-functional requirements
→ Architecture decisions
→ HLD
→ LLD
→ Source code
→ Tests
→ Pull requests
→ Build artifacts
→ Releases
→ Deployments

Every important engineering decision and approval must be traceable.

---

# 3. CORE ARCHITECTURAL DECISIONS

The following architectural decisions have already been made. Treat them as mandatory constraints.

## 3.1 Orchestration: Hybrid

Implement hybrid orchestration.

Use deterministic workflow orchestration for:

- Lifecycle stages.
- Mandatory dependencies.
- Approval gates.
- Workflow state transitions.
- Retry policies.
- Timeout handling.
- Failure recovery.
- Compliance enforcement.
- Release and deployment authorization.

Use intelligent agent orchestration for:

- Task decomposition.
- Agent selection.
- Agent collaboration.
- Context selection.
- Problem analysis.
- Reasoning.
- Delegation.
- Result evaluation.
- Adaptive task execution.

LLMs must not be the authority for enforcing mandatory governance rules.

The deterministic orchestration layer must remain authoritative.

## 3.2 Agent Instantiation: Dynamic

Agents must be dynamically instantiated when required.

Do not design the platform around a fixed collection of permanently running agents.

Implement:

- Agent definitions.
- Agent registration.
- Agent discovery.
- Agent capability descriptions.
- Agent instantiation.
- Agent lifecycle management.
- Agent termination.
- Agent version management.
- Agent execution policies.
- Agent permissions.
- Agent resource limits.

The orchestrator must select agents based on task requirements, available capabilities, policies and execution context.

Agent instantiation must be observable and auditable.

## 3.3 Execution Environment: Isolated

Every agent execution must occur within an appropriately isolated execution boundary.

Support isolation through an extensible execution provider architecture.

Consider:

- Container-based execution.
- Sandboxed execution.
- Process isolation where appropriate.
- Resource quotas.
- Filesystem restrictions.
- Network restrictions.
- Secret isolation.
- Project-level boundaries.
- Execution timeouts.
- Cleanup and termination.

Agents must not receive unrestricted access to the host environment.

The isolation provider must be replaceable without redesigning the orchestration core.

## 3.4 Software Development: Multi-Agent

Use specialized agents that collaborate to complete engineering tasks.

Examples:

- Business Analyst Agent.
- Requirements Engineer Agent.
- Solution Architect Agent.
- HLD Design Agent.
- LLD Design Agent.
- Software Developer Agent.
- Code Reviewer Agent.
- Test Engineer Agent.
- Security Engineer Agent.
- DevOps Agent.
- Release Manager Agent.

Agents must have clearly defined responsibilities and permissions.

Do not create agents simply to increase agent count.

Use the minimum effective set of agents required for each task.

## 3.5 Human Approvals: Gated

Human approvals are mandatory at defined governance boundaries.

At minimum:

- HLD approval before LLD generation.
- LLD approval before code generation.
- Required pull-request approvals before protected-branch merges.
- Production release/deployment authorization.

Approval rules must be enforced by deterministic workflow and authorization services.

An agent must never approve its own work.

Agents must never bypass an approval gate by invoking another API, tool or workflow directly.

Approval decisions must be recorded with:

- Approver identity.
- Approval role.
- Timestamp.
- Decision.
- Comments.
- Related artifact version.
- Workflow identifier.
- Audit event.

Rejected artifacts must return to the appropriate revision stage without losing previous context or history.

---

# 4. TECHNOLOGY AND IMPLEMENTATION STRATEGY

## 4.1 Rust-First Architecture

Use Rust as the primary implementation language for the EAEH core.

Rust should be used for:

- Core runtime.
- Orchestration engine.
- Agent lifecycle management.
- Execution coordination.
- Policy enforcement.
- Workflow state management.
- API services.
- Tool execution coordination.
- Event processing.
- Audit and observability infrastructure where appropriate.

Choose mature Rust libraries and frameworks based on suitability, stability, security and maintainability.

Evaluate suitable technologies such as:

- Tokio for asynchronous execution.
- Axum for HTTP services.
- Serde for serialization.
- SQLx for database access.
- Tracing for instrumentation.
- OpenTelemetry integration for distributed observability.
- Established authentication and authorization libraries.
- Appropriate container and process-management integrations.

These are candidates, not mandatory dependencies. Evaluate before selecting.

Do not introduce a dependency without a clear architectural justification.

## 4.2 AI and LLM Integration

Do not attempt to implement an LLM inference engine in Rust.

Build a provider-independent LLM integration layer.

Support multiple providers through adapters.

The design should accommodate:

- Commercial LLM APIs.
- Cloud-hosted model services.
- Locally hosted models.
- Future model providers.
- Embedding models.
- Reranking models.

The platform must support model selection based on:

- Task type.
- Capability.
- Context requirements.
- Cost.
- Latency.
- Security classification.
- Data residency.
- Enterprise policies.

Provider-specific implementation details must not leak into the core orchestration engine.

Support model timeouts, retries, fallback policies, token tracking and cost observability.

Never hardcode credentials.

## 4.3 Language-Agnostic Engineering

EAEH must support engineering projects implemented in different programming languages.

Do not build the entire platform around Java or any other specific language.

Avoid an architecture where every new programming language requires extensive modifications to the Rust core.

Implement an extensible validation and engineering tool integration model.

Use a combination of:

- Language-specific tools where appropriate.
- Static analysis tools.
- Compiler diagnostics.
- Test execution tools.
- Security scanners.
- Policy engines.
- LLM-assisted semantic analysis.
- Pluggable validation adapters.

The platform should be able to invoke appropriate tools based on project technology and configured policies.

Do not assume that LLMs replace deterministic validation.

For example:

- Use compilers for compilation correctness.
- Use formatters and linters for formatting and static rules.
- Use test frameworks for execution-based validation.
- Use security scanners for known vulnerabilities.
- Use LLM-assisted analysis for semantic, contextual and architectural assessments.

Support the addition of new languages without modifying the core orchestration architecture.

## 4.4 Extensible Integration Architecture

Integrations must use well-defined interfaces and adapters.

Potential integrations include:

- GitHub.
- GitLab.
- Bitbucket.
- CI/CD systems.
- Jira.
- Confluence.
- ServiceNow.
- Architecture repositories.
- Enterprise document stores.
- Cloud platforms.
- Security scanning tools.
- Observability platforms.
- MCP-compatible tools and servers.

Do not implement every integration in Phase 1.

Design appropriate extension points and implement integrations in the relevant future phases.

---

# 5. HIGH-LEVEL ARCHITECTURAL CAPABILITIES

Design the platform around logically separated capabilities.

The exact module boundaries must be validated during architecture design.

Consider the following capabilities:

## 5.1 Core Runtime

Responsible for:

- Platform initialization.
- Runtime lifecycle.
- Service coordination.
- Configuration.
- Dependency management.
- Health monitoring.

## 5.2 Orchestration Engine

Responsible for:

- Deterministic workflow execution.
- Intelligent task planning.
- Agent delegation.
- Dependency management.
- Workflow state transitions.
- Retry and recovery.
- Approval enforcement.

## 5.3 Agent Management

Responsible for:

- Agent definitions.
- Agent registration.
- Agent discovery.
- Dynamic instantiation.
- Agent lifecycle.
- Capability matching.
- Agent permissions.
- Version management.

## 5.4 Execution Environment Manager

Responsible for:

- Isolated execution provisioning.
- Workspace allocation.
- Resource management.
- Network and filesystem restrictions.
- Secret injection.
- Execution termination.
- Cleanup.

## 5.5 Knowledge Management

Responsible for:

- Enterprise knowledge ingestion.
- Knowledge classification.
- Metadata management.
- Versioning.
- Indexing.
- Retrieval.
- Source tracking.
- Access control.
- Knowledge lifecycle management.

## 5.6 Skills Management

Responsible for:

- Skill registration.
- Skill discovery.
- Skill versioning.
- Skill dependencies.
- Skill validation.
- Skill execution guidance.
- Skill permissions.
- Skill retirement.

## 5.7 Governance and Policy Engine

Responsible for:

- Approval policies.
- Agent permissions.
- Project policies.
- Enterprise standards.
- Security controls.
- Workflow restrictions.
- Policy evaluation.
- Enforcement and audit.

## 5.8 LLM Provider Management

Responsible for:

- Provider configuration.
- Model registration.
- Model selection.
- Routing.
- Fallback.
- Cost tracking.
- Token tracking.
- Provider health.

## 5.9 Engineering Tool Integration

Responsible for:

- External tool adapters.
- MCP integrations.
- Source control integrations.
- CI/CD integrations.
- Security tools.
- Testing tools.
- Cloud integrations.

## 5.10 Observability and Audit

Responsible for:

- Logs.
- Metrics.
- Distributed traces.
- Workflow execution history.
- Agent execution history.
- Tool invocation history.
- Approval records.
- Security events.
- Cost and usage information.

## 5.11 Project and Tenant Management

Responsible for:

- Project registration.
- Project configuration.
- Project-specific knowledge.
- Project-specific skills.
- Project permissions.
- Project isolation.
- Team membership.
- Project lifecycle.

## 5.12 API and User Interaction

Provide suitable interfaces for:

- Platform administrators.
- Enterprise architects.
- Developers.
- Security reviewers.
- Approvers.
- Project managers.

Use CLI and APIs where appropriate.

A web dashboard may be introduced according to the implementation roadmap.

Do not build unnecessary user interfaces before the underlying capabilities are stable.

---

# 6. KNOWLEDGE AND SKILLS MANAGEMENT – FIRST-CLASS CAPABILITIES

Knowledge and skills must be treated as fundamental platform capabilities.

Do not treat them as prompt files scattered throughout the repository.

## 6.1 Enterprise Knowledge

Support knowledge from:

- BRDs.
- Business processes.
- Business rules.
- Domain definitions.
- HLDs.
- LLDs.
- Architecture principles.
- Architecture decision records.
- API specifications.
- Data models.
- Security standards.
- Cloud governance standards.
- Coding standards.
- Technology standards.
- Engineering practices.
- Reference implementations.
- Approved libraries and frameworks.
- Testing standards.
- CI/CD standards.
- Compliance requirements.

Knowledge must support:

- Source registration.
- Ingestion.
- Parsing.
- Classification.
- Metadata extraction.
- Versioning.
- Validation.
- Indexing.
- Retrieval.
- Access control.
- Updates.
- Retirement.
- Provenance.
- Traceability.

Support different knowledge scopes:

1. Enterprise-wide.
2. Business domain.
3. Technology domain.
4. Project.
5. Team.
6. Individual workflow where permitted.

Do not allow project-specific knowledge to leak into unrelated projects.

## 6.2 Agent Skills

Skills should represent reusable engineering capabilities and domain-specific instructions.

Examples:

- BRD analysis.
- Requirements decomposition.
- HLD generation.
- HLD compliance assessment.
- LLD generation.
- Java development.
- Spring Boot development.
- API design.
- Database design.
- Security assessment.
- Code review.
- Unit testing.
- Integration testing.
- CI/CD configuration.
- Cloud architecture assessment.

Skills must support:

- Registration.
- Versioning.
- Metadata.
- Dependencies.
- Required knowledge.
- Required tools.
- Input and output contracts.
- Execution constraints.
- Permissions.
- Validation.
- Retirement.

Agents should discover and use skills dynamically based on task requirements.

Avoid tightly coupling skills to one specific agent.

## 6.3 Knowledge and Skill Traceability

For every important agent execution, record:

- Knowledge sources retrieved.
- Knowledge versions.
- Skills selected.
- Skill versions.
- Model and provider.
- Tool invocations.
- Applicable policies.
- Resulting artifacts.

Generated engineering artifacts must be traceable to their source requirements, knowledge and applicable standards.

---

# 7. MULTI-PROJECT AND ENTERPRISE DESIGN

Design EAEH for multiple projects and teams.

Each project must be independently configurable.

Project configuration should include:

- Project identity.
- Business domain.
- Technology stack.
- Architecture style.
- Git repositories.
- Engineering standards.
- Project knowledge.
- Approved skills.
- Allowed agents.
- Model policies.
- Security requirements.
- Execution limits.
- Approval roles.
- Workflow configuration.

Support enterprise-wide standards inherited by projects, with controlled project-specific extensions.

Do not allow a project to override mandatory enterprise security or governance policies.

Design for future multi-tenancy without unnecessarily implementing distributed enterprise infrastructure in Phase 1.

---

# 8. SECURITY AND GOVERNANCE PRINCIPLES

Security and governance must be designed into the platform from the beginning.

Mandatory requirements:

- Authentication.
- Role-based access control.
- Project-level authorization.
- Least privilege.
- Secure secrets management.
- Execution isolation.
- Data protection.
- Secure communication.
- Audit logging.
- Policy enforcement.
- Approval integrity.
- Input validation.
- Output validation.
- Dependency security.
- Tool authorization.
- Network restrictions.
- Resource quotas.

Consider prompt injection, malicious documents, compromised skills, malicious tool responses and untrusted repository content.

Treat external content as untrusted data, not instructions that can override system policies.

An agent must never gain additional privileges merely because a document, prompt or tool response requests them.

The policy engine must remain independent of LLM decisions.

Use defense in depth.

---

# 9. IMPLEMENTATION PHASES

Implement EAEH incrementally through the following eight phases.

Do not combine phases merely to accelerate delivery.

Every phase must produce working software, automated tests, documentation and a phase completion report.

## PHASE 1 – CORE HARNESS FOUNDATION

### Objective

Build the foundational Rust-first runtime and execution infrastructure.

### Scope

Implement:

1. Rust project foundation and workspace structure.
2. Core runtime.
3. Configuration management.
4. Logging and error handling.
5. LLM provider abstraction and initial provider adapter.
6. Hybrid orchestration foundation.
7. Deterministic workflow engine foundation.
8. Agent definition and registration foundation.
9. Agent discovery.
10. Dynamic agent instantiation foundation.
11. Agent lifecycle management.
12. Multi-agent communication foundation.
13. Isolated execution provider abstraction and initial implementation.
14. Execution resource limits.
15. Basic policy enforcement foundation.
16. Human approval gate foundation.
17. Workflow persistence.
18. Workflow recovery foundation.
19. Audit and observability foundation.
20. CLI and/or REST API for core platform operations.
21. Automated testing infrastructure.

### Important boundaries

Phase 1 is NOT expected to deliver:

- Complete enterprise knowledge management.
- Complete skills management.
- BRD analysis.
- HLD generation.
- LLD generation.
- Production code generation workflows.
- Complete software testing orchestration.
- Git and CI/CD lifecycle automation.
- Production release management.

However, Phase 1 must provide the necessary architectural extension points for these capabilities.

### Phase 1 acceptance criteria

Demonstrate that:

- EAEH starts successfully.
- Configuration works.
- LLM connectivity works through the provider abstraction.
- Agents can be registered and discovered.
- Agents can be dynamically instantiated.
- Multiple agents can collaborate.
- Agents execute within isolated environments.
- Workflow state persists.
- Interrupted workflows can recover.
- Human approval gates are enforced.
- Unauthorized actions are rejected.
- Audit records are generated.
- Core APIs/CLI work.
- Automated tests pass.

### Expected deliverables

- Working Rust-first runtime.
- Core orchestration foundation.
- Agent lifecycle management.
- Isolated execution foundation.
- Governance foundation.
- Persistence and observability.
- Automated test suite.
- Phase 1 completion documentation.

## PHASE 2 – ENTERPRISE KNOWLEDGE AND SKILLS MANAGEMENT

### Objective

Build the enterprise intelligence foundation.

### Scope

Implement:

1. Knowledge source registration.
2. Document ingestion.
3. Document parsing.
4. Metadata extraction.
5. Knowledge classification.
6. Knowledge versioning.
7. Knowledge indexing.
8. Retrieval architecture.
9. Vector search integration where appropriate.
10. Hybrid retrieval.
11. Reranking integration where appropriate.
12. Knowledge provenance.
13. Knowledge access control.
14. Knowledge lifecycle management.
15. Skill registration.
16. Skill discovery.
17. Skill versioning.
18. Skill dependency management.
19. Skill validation.
20. Skill-to-agent association.
21. Dynamic skill selection.
22. Knowledge and skill execution traceability.
23. Project-level knowledge isolation.
24. Knowledge and skill administration interfaces.

### Architectural requirements

Use a provider-independent retrieval architecture.

Do not tightly couple the core to a specific vector database.

Support future retrieval strategies, including metadata filtering, lexical retrieval, semantic retrieval and hybrid retrieval.

Skills must be reusable across different agent types.

Enterprise knowledge and project knowledge must have clear access boundaries.

### Phase 2 acceptance criteria

Demonstrate that:

- Knowledge sources can be registered.
- Documents can be ingested.
- Knowledge can be versioned.
- Knowledge can be retrieved.
- Access policies are enforced.
- Skills can be registered and versioned.
- Agents can discover and use skills.
- Agents can retrieve relevant knowledge.
- Knowledge and skill usage is traceable.
- Updates and retirement work correctly.

## PHASE 3 – REQUIREMENTS ENGINEERING AND ARCHITECTURE GENERATION

### Objective

Enable EAEH to transform business requirements into approved engineering designs.

### Scope

Implement:

1. BRD ingestion.
2. BRD structural analysis.
3. Business requirement extraction.
4. Functional requirement extraction.
5. Non-functional requirement extraction.
6. Business rule identification.
7. Dependency identification.
8. Ambiguity and conflict detection.
9. Missing requirement identification.
10. Requirements traceability.
11. Requirements validation.
12. Business Analyst Agent.
13. Requirements Engineer Agent.
14. Solution Architect Agent.
15. HLD generation.
16. Architecture diagrams.
17. Architecture decision records.
18. Technology recommendations based on enterprise knowledge.
19. Enterprise standards compliance assessment.
20. Architecture risk analysis.
21. HLD review and approval.
22. LLD generation.
23. API contract generation.
24. Data model generation.
25. Component design.
26. Sequence diagrams.
27. Deployment design.
28. LLD review and approval.
29. Revision and resubmission workflows.
30. Artifact versioning and traceability.

### Mandatory governance

HLD approval must be completed before LLD generation.

LLD approval must be completed before software development begins.

Rejected documents must return to the appropriate revision stage.

Do not allow an agent to bypass approval gates.

### Phase 3 acceptance criteria

Demonstrate that:

- A BRD can be ingested.
- Requirements are extracted.
- Ambiguities and missing requirements are identified.
- Requirements are traceable.
- HLD can be generated.
- HLD can be reviewed and approved.
- LLD can be generated after HLD approval.
- LLD can be reviewed and approved.
- Enterprise knowledge and skills influence the outputs.
- Architecture decisions and sources are traceable.

## PHASE 4 – MULTI-AGENT SOFTWARE DEVELOPMENT

### Objective

Enable governed software development based on approved LLDs.

### Scope

Implement:

1. Development task decomposition.
2. Codebase analysis.
3. Repository context acquisition.
4. Project technology detection.
5. Development agent selection.
6. Dynamic development agent instantiation.
7. Isolated coding workspaces.
8. Code generation.
9. Code modification.
10. Dependency management.
11. Build execution.
12. Compiler integration.
13. Language-specific tool adapters.
14. Coding standard enforcement.
15. Architecture compliance validation.
16. Code review agents.
17. Developer feedback and correction loops.
18. Source-to-requirement traceability.
19. LLD-to-code traceability.

### Language extensibility

Support multiple programming languages through pluggable engineering adapters.

Do not hardcode Java-specific logic into the core runtime.

Use deterministic development tools for syntax, compilation and static checks.

Use LLM-assisted analysis for contextual and semantic assessments.

### Phase 4 acceptance criteria

Demonstrate that:

- Approved LLDs can drive development tasks.
- Relevant agents and skills are selected dynamically.
- Code is generated in isolated environments.
- Project standards are applied.
- Code is traceable to requirements and LLDs.
- Code review is performed.
- Development failures can be handled and retried.

## PHASE 5 – TESTING AND QUALITY ENGINEERING

### Objective

Implement comprehensive software quality validation.

### Scope

Implement:

1. Test planning.
2. Test case generation.
3. Unit test generation.
4. Integration test generation.
5. API testing.
6. Contract testing.
7. Functional testing.
8. Regression testing.
9. Performance testing integration.
10. Security testing integration.
11. Code quality assessment.
12. Coverage reporting.
13. Defect identification.
14. Failure analysis.
15. Automated correction loops where authorized.
16. Test evidence management.
17. Requirements-to-test traceability.
18. Quality gates.

### Important requirements

Use actual testing frameworks and execution tools.

Do not rely exclusively on LLM claims that generated code is correct.

Tests must be executed and their results captured.

Critical quality failures must block progression according to policy.

### Phase 5 acceptance criteria

Demonstrate that:

- Test plans are generated.
- Test cases are generated.
- Tests are executed.
- Results are captured.
- Defects are identified.
- Quality policies are enforced.
- Test coverage and traceability are available.

## PHASE 6 – GIT AND CI/CD INTEGRATION

### Objective

Enable controlled software delivery through source control and automated pipelines.

### Scope

Implement:

1. Git provider abstraction.
2. Repository registration.
3. Branch management.
4. Feature branch creation.
5. Commit management.
6. Pull request creation.
7. Pull request review.
8. Branch protection awareness.
9. Merge approval enforcement.
10. CI/CD integration.
11. Build pipeline execution.
12. Test pipeline execution.
13. Security pipeline integration.
14. Pipeline result collection.
15. Pipeline failure analysis.
16. Artifact registration.
17. Commit and artifact traceability.

Support workflows such as:

Feature → Develop → Master

Do not hardcode a single branching strategy.

Make branching policies configurable per enterprise or project.

### Mandatory governance

Agents may create branches, commits and pull requests within their permissions.

Agents must not bypass protected-branch controls.

Required human approvals must remain enforceable by the source control platform and EAEH governance layer.

### Phase 6 acceptance criteria

Demonstrate that:

- Repositories can be registered.
- Branches can be created.
- Changes can be committed.
- Pull requests can be created.
- CI/CD pipelines can be triggered.
- Pipeline results can be retrieved.
- Required approvals are enforced.
- Branch protection is respected.

## PHASE 7 – RELEASE AND DEPLOYMENT MANAGEMENT

### Objective

Enable governed software release and deployment.

### Scope

Implement:

1. Release planning.
2. Release artifact management.
3. Release versioning.
4. Environment management.
5. Deployment configuration.
6. Deployment approvals.
7. Deployment orchestration.
8. Infrastructure integration.
9. Configuration and secret management.
10. Deployment validation.
11. Smoke testing.
12. Health verification.
13. Rollback orchestration.
14. Release audit.
15. Deployment traceability.
16. Post-deployment verification.

Support environments such as:

Development → QA → Staging → Production

### Mandatory governance

Production deployment must require explicit authorization according to configured policies.

Agents must not independently authorize production deployment.

### Phase 7 acceptance criteria

Demonstrate that:

- Release artifacts can be registered.
- Releases can be prepared.
- Deployments can be initiated.
- Approval policies are enforced.
- Deployment results are recorded.
- Failed deployments can be handled.
- Rollback mechanisms are available and tested.

## PHASE 8 – ENTERPRISE SCALE AND ADVANCED CAPABILITIES

### Objective

Prepare EAEH for enterprise-wide adoption.

### Scope

Implement and improve:

1. Multi-project management.
2. Multi-team management.
3. Enterprise tenant isolation.
4. Advanced role-based access control.
5. Enterprise identity integration.
6. Advanced policy management.
7. Distributed execution.
8. Scalable worker management.
9. Agent capacity management.
10. Advanced model routing.
11. Cost optimization.
12. Enterprise observability.
13. Knowledge federation.
14. Advanced audit and reporting.
15. Enterprise administration.
16. High availability.
17. Disaster recovery.
18. Performance optimization.
19. Operational resilience.
20. Enterprise deployment models.

### Phase 8 acceptance criteria

Demonstrate that:

- Multiple projects operate independently.
- Project data and execution are isolated.
- Enterprise policies are consistently enforced.
- Platform capacity can scale.
- Observability and audit cover enterprise operations.
- Recovery and availability requirements are met.
- Enterprise administration is operational.

---

# 10. CROSS-PHASE ARCHITECTURAL CONSISTENCY

Every implementation phase must preserve the approved architecture.

Before implementing a new phase:

1. Review previous phase completion reports.
2. Review outstanding action items.
3. Review existing architecture decisions.
4. Review dependencies.
5. Inspect the actual source code.
6. Identify required extensions.
7. Identify potential breaking changes.
8. Explain proposed architectural changes.
9. Obtain approval for material architectural deviations.

Do not redesign completed modules unnecessarily.

Do not duplicate existing capabilities.

Do not introduce circular dependencies.

Do not tightly couple unrelated modules.

Prefer clear interfaces and replaceable implementations.

Keep the core runtime independent of domain-specific workflows.

---

# 11. GLOBAL IMPLEMENTATION RULES

These rules apply to every phase.

## 11.1 Inspect Before Implementing

Before changing code:

- Inspect the repository.
- Understand existing modules.
- Read existing documentation.
- Identify current dependencies.
- Review tests.
- Understand existing architecture decisions.

Do not assume the repository is empty.

Do not overwrite existing working implementations without justification.

## 11.2 Architecture Before Code

Before implementing each phase:

1. Explain the phase architecture.
2. Identify affected modules.
3. Explain data flows.
4. Explain interfaces.
5. Explain dependencies.
6. Explain security boundaries.
7. Identify risks.
8. Present implementation tasks.

For material architectural changes, document an ADR.

## 11.3 Incremental Implementation

Implement in small, verifiable increments.

Do not generate large amounts of unverified code.

After each meaningful implementation increment:

- Compile.
- Run relevant tests.
- Validate interfaces.
- Review error handling.
- Check security implications.

## 11.4 Code Quality

Maintain:

- Clear module boundaries.
- Strong typing.
- Explicit error handling.
- Consistent naming.
- Appropriate documentation.
- Testability.
- Maintainability.
- Minimal unnecessary dependencies.

Avoid oversized modules and monolithic services.

## 11.5 No Fake Implementations

Never create placeholder functionality and describe it as completed.

If an external integration is not available:

- Implement the appropriate interface where approved.
- Clearly identify the missing implementation.
- Document how it will be completed.
- Do not report it as operational.

Do not use mock responses as proof of real integration.

Mocks are acceptable for isolated unit tests when explicitly identified.

## 11.6 No Invented Enterprise Standards

Do not invent enterprise architecture, cloud, security or coding standards.

Use supplied and approved sources.

When a required standard is unavailable, identify the gap rather than fabricating a standard.

## 11.7 Configuration and Secrets

Keep environment-specific configuration outside source code.

Never commit secrets, API keys, tokens or passwords.

Provide safe example configuration files.

Use a suitable secrets management mechanism.

## 11.8 Dependency Management

Select dependencies based on:

- Maturity.
- Security.
- Compatibility.
- Licensing.
- Maintenance.
- Performance.
- Architectural suitability.

Document significant dependency choices.

---

# 12. MANDATORY STAGE COMPLETION DOCUMENTATION

At the end of EVERY phase, produce a dedicated Markdown completion document.

This is a mandatory implementation deliverable.

Create:

`docs/implementation/stages/PHASE-<NN>-COMPLETION.md`

Examples:

- PHASE-01-COMPLETION.md
- PHASE-02-COMPLETION.md
- PHASE-03-COMPLETION.md
- PHASE-04-COMPLETION.md
- PHASE-05-COMPLETION.md
- PHASE-06-COMPLETION.md
- PHASE-07-COMPLETION.md
- PHASE-08-COMPLETION.md

Do not overwrite previous phase completion reports.

## 12.1 Required completion document sections

Every completion document must contain:

### A. Executive Summary

- Phase objective.
- Expected deliverables.
- Actual deliverables.
- Implementation status.
- Acceptance status.
- Important limitations.

### B. Architecture and Implementation Summary

- Components introduced.
- Components modified.
- Responsibilities.
- Communication.
- Data flows.
- Architectural decisions.
- Security considerations.
- Architecture diagrams where useful.

### C. Implementation Inventory

- New files.
- Modified files.
- Modules.
- APIs.
- CLI commands.
- Database changes.
- Configuration changes.
- Dependencies.
- Tests.
- Documentation.

### D. Capabilities and Acceptance Criteria

Map every planned capability to actual implementation.

Use:

- IMPLEMENTED AND VERIFIED.
- IMPLEMENTED BUT NOT VERIFIED.
- PARTIALLY IMPLEMENTED.
- NOT IMPLEMENTED.
- BLOCKED.

### E. Step-by-Step Testing Guide

This section is mandatory.

For every significant capability, provide:

1. Test ID.
2. Test name.
3. Objective.
4. Prerequisites.
5. Environment requirements.
6. Sample test data.
7. Exact commands.
8. API requests or CLI operations.
9. Expected outputs.
10. Expected system behaviour.
11. PASS/FAIL criteria.
12. Logs and evidence to inspect.
13. Troubleshooting instructions.

Include relevant unit, integration, functional, security, authorization, recovery, performance and end-to-end tests.

Use only actual implemented commands and interfaces.

Never invent commands or endpoints.

### F. Actual Test Execution Results

Provide:

| Test ID | Test description | Result | Evidence | Defect |
|---|---|---|---|---|

Report:

- Total tests.
- Passed.
- Failed.
- Blocked.
- Not tested.
- Coverage where measurable.
- Outstanding defects.

Never fabricate test results.

### G. How to Use the Implemented Capabilities

Explain:

- Installation.
- Configuration.
- Startup.
- Shutdown.
- CLI.
- APIs.
- Sample requests.
- Sample responses.
- Operational procedures.
- What can be used now.
- What belongs to future phases.

### H. Security and Governance

Document:

- Authentication.
- Authorization.
- Isolation.
- Secrets.
- Audit.
- Approval enforcement.
- Security test results.
- Known security limitations.

### I. Known Limitations and Technical Debt

Identify:

- Incomplete functionality.
- Defects.
- Deferred decisions.
- Performance limitations.
- Security risks.
- Workarounds.
- Architectural concerns.

### J. Next Implementation Stage

Explain:

- Next phase objective.
- Expected functionality.
- Dependencies.
- Architectural prerequisites.
- Components to extend.
- Components to introduce.
- Integration requirements.
- Acceptance criteria.

### K. Action Items

Use:

| ID | Action | Description | Priority | Dependency | Responsible role | Target phase | Status |
|---|---|---|---|---|---|---|---|

Classify actions as:

- Mandatory before next phase.
- Recommended improvements.
- Deferred technical debt.
- Operational activities.
- Future enhancements.

Carry forward unresolved action items.

### L. Phase Exit Decision

Use one of:

- READY FOR NEXT PHASE.
- READY WITH ACCEPTED EXCEPTIONS.
- NOT READY – ACTION REQUIRED.
- BLOCKED.

Explain the decision.

A critical security, governance or acceptance failure must prevent an unconditional READY decision.

---

# 13. CROSS-PHASE DOCUMENTATION

Maintain the following documents throughout implementation.

## 13.1 Roadmap

File:

`docs/implementation/ROADMAP.md`

Maintain:

- All phases.
- Objectives.
- Capabilities.
- Dependencies.
- Acceptance criteria.
- Status.
- Completion report links.
- Next steps.

## 13.2 Action Item Register

File:

`docs/implementation/ACTION-ITEMS.md`

Maintain a consolidated register of actions from all phases.

Never silently remove unresolved actions.

Record when an item is completed or formally deferred.

## 13.3 Test Strategy

File:

`docs/implementation/TEST-STRATEGY.md`

Maintain:

- Testing architecture.
- Test categories.
- Test environments.
- Test data strategy.
- Security testing.
- Integration testing.
- End-to-end testing.
- Regression testing.
- Test evidence management.

## 13.4 Architecture Documentation

Maintain the actual architecture documentation under:

`docs/architecture/`

Include:

- Architecture overview.
- Component architecture.
- Runtime architecture.
- Agent architecture.
- Orchestration architecture.
- Execution isolation architecture.
- Knowledge architecture.
- Skills architecture.
- Security architecture.
- Data architecture.
- Integration architecture.
- Architecture decision records.

Update these documents whenever implementation materially changes the architecture.

---

# 14. MANDATORY PHASE COMPLETION PROCESS

At the end of each phase:

1. Stop implementation.
2. Review approved phase scope.
3. Inspect the actual implementation.
4. Verify architecture compliance.
5. Identify missing functionality.
6. Execute automated tests.
7. Execute phase-specific functional tests.
8. Execute relevant security tests.
9. Record actual results.
10. Resolve defects within the approved scope.
11. Rerun affected and regression tests.
12. Update architecture documentation.
13. Generate the phase completion Markdown.
14. Update the roadmap.
15. Update the action-item register.
16. Prepare the next phase implementation plan.
17. Present the phase exit decision.
18. Wait for explicit user authorization.

Do not automatically begin the next phase.

Implementation completion, phase acceptance and authorization to proceed are separate events.

---

# 15. PHASE DEFINITION OF DONE

A phase is complete only when:

1. Approved scope has been implemented.
2. Required components exist.
3. Acceptance criteria have been verified.
4. Required automated tests pass.
5. Mandatory security and governance tests pass.
6. No unresolved critical defects remain.
7. Actual testing instructions are documented.
8. Actual test results are recorded.
9. Operational instructions are documented.
10. Phase completion Markdown is generated.
11. Architecture documentation is updated.
12. Roadmap is updated.
13. Action items are updated.
14. Next phase plan is documented.
15. Phase exit decision is presented.

A phase must never be declared complete merely because source code was generated.

---

# 16. EXECUTION STRATEGY FOR CLAUDE CODE

Follow a controlled phased implementation strategy.

## Step 1 – Initial Repository Assessment

Inspect the existing repository.

Determine:

- Current structure.
- Existing code.
- Existing architecture.
- Existing tests.
- Dependencies.
- Build system.
- Development environment.

Do not modify anything during the initial assessment.

## Step 2 – Architecture and Implementation Planning

Prepare:

- Proposed architecture.
- Module breakdown.
- Technology decisions.
- Repository structure.
- Phase dependencies.
- Implementation plan.
- Risks.
- Initial architecture decision records.

Explain how the selected architecture supports all eight phases.

## Step 3 – Implement Phase 1 Only

Do not implement Phase 2 or later functionality.

Implement Phase 1 according to its scope and acceptance criteria.

Keep future phases in mind when designing interfaces and extension points.

## Step 4 – Test Phase 1

Execute the Phase 1 test strategy.

Demonstrate:

- Runtime.
- Agent registration.
- Dynamic instantiation.
- Multi-agent collaboration.
- Isolated execution.
- Approval enforcement.
- Persistence.
- Recovery.
- Auditability.
- CLI/API functionality.

## Step 5 – Generate Phase 1 Completion Package

Generate:

`docs/implementation/stages/PHASE-01-COMPLETION.md`

Update:

- `docs/implementation/ROADMAP.md`
- `docs/implementation/ACTION-ITEMS.md`
- `docs/implementation/TEST-STRATEGY.md`

Provide actual commands and step-by-step instructions for operating and testing Phase 1.

## Step 6 – Present Phase 1 Handover

Explain:

- What has been implemented.
- How to start the platform.
- How to use its current functionality.
- How to execute tests.
- What is not yet supported.
- Outstanding issues.
- What Phase 2 will deliver.
- What I need to do next.

## Step 7 – Wait for Authorization

Do not start Phase 2 until I explicitly authorize it.

Repeat the same process for every subsequent phase.

---

# 17. EXPECTED FIRST RESPONSE

Before writing implementation code, respond with:

1. Your understanding of the EAEH vision.
2. The proposed high-level architecture.
3. How the five mandatory architectural decisions are addressed.
4. The proposed Rust workspace and repository structure.
5. Major component responsibilities.
6. LLM integration architecture.
7. Dynamic agent lifecycle design.
8. Execution isolation design.
9. Knowledge and skills management architecture.
10. Security and governance architecture.
11. Data and persistence architecture.
12. Observability architecture.
13. Eight-phase implementation roadmap.
14. Phase dependencies.
15. Key architecture risks and mitigation strategies.
16. Phase 1 implementation plan.
17. Phase 1 acceptance criteria.
18. Phase 1 test strategy.

Clearly identify architectural decisions that still require my approval.

Do not begin Phase 1 implementation until the initial architecture and implementation plan have been presented and approved.

---

# 18. FINAL INSTRUCTIONS

Build EAEH as a serious enterprise engineering platform.

Prioritize:

- Architectural integrity.
- Rust-first implementation.
- Hybrid orchestration.
- Dynamic agent instantiation.
- Isolated execution.
- Multi-agent collaboration.
- Mandatory human governance.
- Enterprise knowledge management.
- Reusable skill management.
- Language-independent engineering.
- Security by design.
- Full traceability.
- Extensibility.
- Operational reliability.
- Testability.
- Maintainability.

Do not optimize for producing the largest amount of code in the shortest time.

Optimize for building a platform that can evolve safely across all eight phases.

Always distinguish implemented functionality from planned functionality.

Always provide evidence for completion.

Always document how to test and use each phase.

Never proceed to the next phase without explicit authorization.

Your immediate task is to assess the repository, present the proposed architecture and prepare the Phase 1 implementation plan. Do not start coding until the architecture is approved.
