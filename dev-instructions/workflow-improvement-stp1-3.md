# EAEH Workflow Management – Comprehensive Usability, Reliability and Developer Experience Enhancement

## 1. Objective

Read the repository's `claude-instructions.md` completely before making any changes.

Improve EAEH workflow management to deliver a professional, reliable and intuitive developer experience.

The current workflow management experience is unnecessarily difficult. Developers encounter confusing authentication and authorization errors, worker startup problems, unclear workflow states, manual recovery steps, inconsistent command behaviour and difficulty understanding why workflows are waiting or blocked.

The objective is to make EAEH workflow management straightforward:

- Developers should be able to start a workflow without manually troubleshooting worker configuration.
- EAEH should automatically identify the correct developer identity.
- Workers should reliably execute eligible tasks.
- Workflow progress should be visible and understandable.
- Human approval should be straightforward.
- Failed and interrupted workflows should be recoverable.
- Errors should explain the actual cause and provide actionable remediation.
- CLI and Governance Portal should provide a consistent experience.

This is an engineering improvement, not merely a documentation exercise.

## 2. Mandatory scope restrictions

**Implementation must affect only functionality within Phases 1, 2 and 3.**

Do not implement Phase 4, 5, 6, 7 or 8 functionality.

Do not change the existing eight-phase roadmap.

### Strict protection of `claude-instructions.md`

Treat the existing master instructions as authoritative.

Do not:
- Modify existing phase definitions.
- Change implementation sequencing or next steps.
- Change phase dependencies.
- Change acceptance criteria.
- Reorder, remove or rename phases.
- Reinterpret existing architectural decisions.
- Introduce new future-phase implementation requirements.
- Modify existing roadmap content to accommodate this enhancement.

If additional instructions are needed, create a supplementary implementation document.

All existing instructions and roadmap content must remain intact.

## 3. Begin with a complete assessment

Before implementation, inspect the existing codebase.

Review:

- Workflow engine and workflow state machine.
- Workflow definitions and configuration.
- Workflow execution and orchestration.
- Task creation and scheduling.
- Worker lifecycle and worker registration.
- Worker authentication and identity resolution.
- Principal and role management.
- Project-level authorization.
- Token issuance and credential storage.
- External AI execution integration.
- Claude Code and GitHub Copilot integration.
- MCP tools and interfaces.
- Approval lifecycle and approval APIs.
- Artifact generation and version management.
- Workflow persistence and recovery.
- CLI commands and command help.
- Governance Portal frontend and backend APIs.
- Existing audit and observability mechanisms.
- Existing automated tests.

Identify the actual root causes of workflow management problems.

Do not implement workarounds without understanding the underlying architecture.

Produce a short findings document before implementation, identifying architectural weaknesses, functional gaps and proposed changes.

## 4. Authentication and authorization improvements

Authentication and authorization must be reliable and predictable.

### 4.1 Developer identity management

Inspect how named credential profiles map to principals and roles.

Provide clear separation between:

- Admin identity.
- Developer/operator identity.
- Approver identity.
- Knowledge steward identity.

A profile name must not create the impression that it represents a particular role when its underlying principal has different permissions.

The CLI should clearly display:

- Profile name.
- Principal ID.
- Principal name.
- Assigned roles.
- Project access.
- Credential validity.
- Default profile.
- Whether the profile can perform the requested operation.

### 4.2 Identity resolution

Establish a consistent identity-resolution mechanism across:

- CLI commands.
- Background workers.
- Workflow execution.
- MCP operations.
- Approval operations.

Avoid situations where the CLI reports that a developer has access, but the background worker cannot execute the same project's task.

The identity used to initiate a workflow and the identity used by the worker must be handled consistently.

If a worker operates under a separate service identity, make that explicit and enforce the required authorization model.

Never silently substitute an administrator identity for a developer identity.

### 4.3 Authorization diagnostics

When authorization fails, provide useful information:

- Which principal was resolved?
- Which profile was used?
- Which project is involved?
- Which permission is required?
- Which role is required?
- Does the principal have project access?
- Is the credential valid?
- Is the failure due to identity resolution, project membership, role assignment or policy enforcement?

Provide the exact remediation command only when it is supported by the actual CLI.

Do not expose tokens, credentials or sensitive authentication information.

### 4.4 Security

Do not weaken authorization to make workflow execution easier.

Do not grant administrator privileges automatically.

Preserve tenant isolation, project isolation, role-based access control and auditability.

## 5. Worker lifecycle management

Worker management must become a first-class EAEH capability.

Inspect and improve:

- Worker discovery.
- Worker registration.
- Worker startup.
- Worker shutdown.
- Worker health.
- Worker identity.
- Worker availability.
- Task claiming.
- Task execution.
- Task completion.
- Worker recovery.
- Worker lease expiration.
- Stale worker detection.

### Expected behaviour

1. When a workflow is started, EAEH should determine whether a suitable worker is available.
2. If the worker is not running, EAEH should provide a clear explanation and, where safe and supported, start it automatically.
3. The worker should use the correct authorized identity.
4. The worker should claim only eligible tasks.
5. The worker should not execute tasks belonging to unauthorized projects.
6. A worker should not execute the same task multiple times because of duplicate polling or recovery.
7. Interrupted tasks should be recoverable using persistent execution state.
8. Worker shutdown should not corrupt workflow state.
9. Worker health and execution status should be observable.

Avoid requiring developers to manually run multiple commands to start an ordinary workflow.

Do not create unbounded background processes or automatic restart loops.

Provide bounded retries, sensible timeouts, cancellation and explicit failure states.

## 6. Workflow lifecycle management

Establish one consistent and well-defined workflow lifecycle.

Inspect existing states and transitions before introducing or changing anything.

The lifecycle should clearly distinguish:

- Created.
- Queued.
- Running.
- Waiting for external AI.
- Waiting for human approval.
- Waiting for a worker.
- Failed.
- Blocked.
- Cancelled.
- Completed.

Use existing state definitions where possible.

Do not introduce duplicate or contradictory workflow states.

### Workflow execution

A developer should be able to:

1. Select a project.
2. Verify project readiness.
3. Select a registered workflow.
4. Start the workflow.
5. Monitor execution.
6. Review generated artifacts.
7. Approve or request changes at approval gates.
8. Resume execution automatically after approval.
9. View the final outcome and execution history.

The workflow engine should automatically manage eligible transitions.

Do not require developers to manually trigger the next stage when the workflow engine can safely do this.

### Workflow commands

Review the existing CLI and provide a consistent command experience for:

- Listing workflows.
- Starting workflows.
- Viewing workflow status.
- Watching workflow execution.
- Viewing workflow details.
- Viewing pending tasks.
- Viewing worker status.
- Viewing pending approvals.
- Cancelling workflows.
- Resuming recoverable workflows.
- Inspecting workflow execution history.

Do not blindly introduce new commands if equivalent commands already exist.

Improve command discoverability, help text, validation and error messages.

Where appropriate, provide concise output by default and structured JSON output for automation.

## 7. Workflow monitoring and diagnostics

A developer must never have to guess what EAEH is doing.

Provide clear visibility into:

- Workflow ID.
- Project.
- Workflow definition and version.
- Current stage.
- Current task.
- Assigned agent.
- Worker identity.
- Task execution status.
- Task start time.
- Last activity.
- Duration.
- Retry count.
- Dependencies.
- Approval requirements.
- Blocking reason.
- Next expected action.

The monitoring experience must explain whether the workflow is:

- Actively progressing.
- Waiting for external AI.
- Waiting for human approval.
- Waiting for worker availability.
- Blocked by missing authorization.
- Blocked by missing configuration.
- Failed due to execution error.

Provide meaningful explanations rather than generic messages such as "task pending" or "worker unavailable".

### Watching workflows

Improve `eaeh workflow watch` so that:

- It displays meaningful stage transitions.
- It shows why execution is waiting.
- It displays task-level progress.
- It handles terminal states cleanly.
- It reconnects safely after temporary interruptions.
- It does not lose workflow state when the terminal closes.
- It provides a clear indication of the next required action.

Watching a workflow must remain optional; execution must continue independently.

## 8. External AI execution integration

EAEH uses external AI execution through Claude Code and GitHub Copilot.

Preserve the external AI architecture.

EAEH remains the deterministic governance and orchestration control plane.

External AI clients perform model reasoning and generation using the user's existing subscription and supported authentication mechanisms.

EAEH must not silently invoke a separately billed LLM API.

Improve the handoff lifecycle:

1. EAEH identifies an eligible AI task.
2. EAEH prepares the task and project context.
3. The authorized external AI client retrieves or receives the task.
4. The external AI client performs the work.
5. The generated result is submitted to EAEH.
6. EAEH validates the result.
7. EAEH persists the artifact and audit information.
8. EAEH advances the workflow when all conditions are satisfied.

Handle:

- Missing external AI clients.
- Invalid client authentication.
- Missing MCP configuration.
- External AI execution failures.
- Task timeouts.
- Duplicate submissions.
- Invalid output.
- Interrupted handoffs.
- Worker restarts.
- Expired task leases.

Do not assume that EAEH can access a Claude Code or GitHub Copilot internal session unless an actual supported integration exists.

Do not introduce direct Anthropic API calls, Agent SDK billing or API-key-based fallback into the subscription-based execution path.

Verify the actual Claude Code CLI authentication behaviour in the supported environment. If subscription authentication cannot be verified, fail safely and explain the limitation.

## 9. Approval management

Human approval is a critical governance capability.

Improve approval handling for the existing Phase 1–3 workflow gates, including requirements, HLD and LLD where applicable.

The developer and approver should be able to:

- See pending approvals.
- Open the complete artifact.
- Review artifact version and metadata.
- Understand the current workflow stage.
- See validation results.
- Review traceability.
- Approve.
- Request changes.
- Reject.
- Provide comments.

Ensure:

- Approval decisions are enforced by the backend.
- Approvals reference the correct artifact version and hash.
- Stale approvals cannot approve modified artifacts.
- Rejection and change requests are recorded.
- Approved artifacts are immutable or versioned appropriately.
- Workflow progression occurs automatically after successful approval.

Do not bypass approval gates.

## 10. Governance Portal enhancements

Improve the existing Governance Portal to provide a professional workflow operations experience.

The portal must use actual EAEH backend data.

Do not introduce mock workflow statistics or simulated task execution.

### Dashboard

Display:

- Total projects.
- Active workflows.
- Running tasks.
- Tasks waiting for external AI.
- Pending approvals.
- Failed workflows.
- Blocked workflows.
- Completed workflows.

Provide an active workflow table containing:

- Project.
- Workflow.
- Current stage.
- Current task.
- Assigned agent.
- Status.
- Last activity.
- Duration.
- Required action.

Users should be able to open a workflow and understand exactly what is happening.

### Workflow details

Provide a detailed workflow view with:

- Workflow lifecycle.
- Stage progression.
- Task execution details.
- Agent assignment.
- Worker health.
- External AI handoff status.
- Approval history.
- Generated artifacts.
- Execution logs.
- Error details.
- Audit history.

### Operational actions

Expose only actions supported by the backend and authorized for the current identity.

Examples:

- Start workflow.
- View status.
- View execution details.
- Resume eligible workflows.
- Cancel eligible workflows.
- Review approvals.
- View worker health.

Require confirmation for destructive or consequential operations.

### Initial authentication scope

Preserve the existing Phase 1–3 portal scope: no authentication UI in this initial local-development implementation.

Do not add login, registration, OIDC or a new user-management system as part of this enhancement.

However, do not weaken backend authorization.

Document the requirement for authentication before exposing the portal outside a trusted local-development environment.

## 11. Error handling and self-diagnostics

Create a consistent error model across CLI, worker, workflow engine and portal.

Every operational error should explain:

1. What happened?
2. Why did it happen?
3. Which workflow, task or project was affected?
4. Is the workflow recoverable?
5. What should the developer do next?
6. Which exact command should be used, if applicable?

Introduce diagnostic capabilities where appropriate.

A developer should be able to run a diagnostic command that checks:

- Current project context.
- Current identity.
- Credential validity.
- Project authorization.
- Workflow registration.
- Workflow readiness.
- Worker availability.
- External AI configuration.
- MCP configuration.
- Pending tasks.
- Pending approvals.
- Persistence health.

Diagnostics must distinguish blocking errors from warnings.

Do not claim that a workflow is ready if an essential execution dependency is unavailable.

## 12. Reliability and persistence

Ensure workflow execution survives:

- Terminal closure.
- CLI interruption.
- Worker restart.
- Application restart.
- Temporary external AI unavailability.
- Network interruption.
- Duplicate task polling.
- Duplicate result submission.

Use persistent workflow state.

Implement idempotent task execution and result submission.

Use bounded retries and explicit recovery policies.

Avoid:

- Lost tasks.
- Duplicate execution.
- Duplicate artifacts.
- Incorrect workflow transitions.
- Silent failures.
- Infinite retry loops.
- Stale worker leases.
- Uncontrolled task concurrency.

Preserve the existing persistence architecture. Do not introduce a separate database for workflow management without a demonstrated architectural requirement.

## 13. Testing requirements

Create meaningful automated tests for the actual implementation.

At minimum, cover:

### Identity and authorization
- Correct developer profile resolution.
- Incorrect profile-to-principal mapping.
- Expired and revoked credentials.
- Operator access to authorized projects.
- Operator access denied to unauthorized projects.
- Admin and developer identities remain separate.
- Worker uses the correct identity.

### Worker lifecycle
- Worker startup.
- Worker shutdown.
- Worker unavailable.
- Worker restart.
- Stale worker detection.
- Duplicate task claim prevention.
- Interrupted task recovery.

### Workflow execution
- Successful workflow execution.
- Stage transitions.
- Waiting for external AI.
- Waiting for approval.
- Approval and automatic continuation.
- Rejection and controlled correction.
- Task failure.
- Retry exhaustion.
- Cancellation.
- Restart recovery.

### External AI
- Task handoff.
- Result submission.
- Invalid result.
- Duplicate result.
- External AI unavailable.
- Authentication failure.
- No hidden direct-provider fallback.

### Governance Portal
- Dashboard reflects backend data.
- Workflow detail reflects actual execution.
- Approval actions work.
- Errors are understandable.
- Unauthorized actions are rejected.

Run the relevant existing test suites and regression tests.

Do not report tests as successful unless they were actually executed.

## 14. Documentation

Review all existing user-facing documentation.

Update the relevant guides in place, particularly:

- `docs/user-guides/EAEH-GETTING-STARTED.md`
- `docs/user-guides/DEVELOPER-EXPERIENCE-GUIDE.md`
- CLI documentation.
- Workflow management documentation.
- Worker management documentation.
- Authentication and authorization documentation.
- MCP integration documentation.
- Governance Portal documentation.

Correct obsolete commands and confusing manual procedures.

Provide a clear end-to-end workflow management guide showing the developer's actual operating experience.

Create:

`docs/implementation/EAEH-WORKFLOW-MANAGEMENT-ENHANCEMENTS.md`

Document:

- Root causes discovered.
- Architectural changes.
- CLI improvements.
- Worker lifecycle improvements.
- Identity and authorization improvements.
- Workflow state management.
- Recovery mechanisms.
- Portal enhancements.
- Testing evidence.
- Known limitations.
- Remaining risks.

**Do not modify the existing next steps or roadmap in `claude-instructions.md`.**

## 15. End-to-end acceptance scenario

Use the existing `simple-springboot-api-a` project, if its current state permits, to validate the improvements.

The application scope is:

- Java 21.
- Spring Boot 3.x.
- Maven.
- GET `/api/hello`.
- GET `/api/info`.
- GET `/api/products`.
- Three static in-memory products.
- No database.
- No Spring Security.
- No authentication.

Validate the existing Phase 1–3 engineering design workflow:

1. Developer identity is correctly resolved.
2. Project readiness is correctly reported.
3. Workflow starts.
4. Worker becomes available with the correct identity.
5. BRD analysis executes.
6. Requirements artifact is generated.
7. Workflow waits for requirements approval.
8. Approver reviews and approves.
9. HLD generation executes.
10. Workflow waits for HLD approval.
11. Approver reviews and approves.
12. LLD generation executes.
13. Workflow waits for LLD approval.
14. Approver reviews and approves.
15. Workflow reaches its expected Phase 3 terminal state.
16. Artifacts and audit history are persisted.
17. CLI and portal show consistent results.

Do not falsely claim that the entire scenario passed if some steps could not be executed.

If external AI authentication, worker authorization or an environment dependency prevents testing, document the exact limitation and the evidence.

Do not damage or overwrite existing project artifacts or historical workflow executions during testing.

## 16. Implementation principles

- Preserve the existing Rust-first architecture.
- Preserve deterministic governance and workflow enforcement.
- Preserve external AI execution architecture.
- Prefer improving existing services over introducing duplicate components.
- Keep implementation modular and maintainable.
- Avoid unnecessary dependencies.
- Preserve backward compatibility where feasible.
- Maintain tenant and project isolation.
- Ensure auditability.
- Never expose credentials in logs.
- Do not bypass authorization or human approval.
- Do not introduce functionality belonging to future phases.

## 17. Execution and completion criteria

Work systematically:

1. Inspect existing architecture and implementation.
2. Identify and document root causes.
3. Present the proposed implementation approach.
4. Implement the approved in-scope changes.
5. Add and execute automated tests.
6. Run regression tests.
7. Update documentation.
8. Validate the end-to-end Phase 1–3 scenario.
9. Produce a final implementation report.

The final report must contain:

- Problems identified.
- Root causes.
- Changes implemented.
- Files changed.
- CLI changes.
- Worker lifecycle changes.
- Authentication and authorization changes.
- Workflow management changes.
- Governance Portal changes.
- Test results with actual evidence.
- End-to-end acceptance results.
- Remaining limitations.
- Security considerations.
- Documentation updated.
- Explicit confirmation that Phases 4–8 and the existing master roadmap were left unchanged.

Stop after completing this enhancement.

Do not proceed with Phase 4 or any later phase.

Do not commit or push changes unless explicitly authorized.

**Definition of success:** A developer should be able to start, monitor, approve, recover and complete a Phase 1–3 EAEH workflow without manually troubleshooting identity mappings, worker startup, authorization inconsistencies or unclear workflow states.