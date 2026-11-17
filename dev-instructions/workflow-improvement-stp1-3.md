# EAEH — Project Onboarding, Workflow Recovery and Complete Governance Portal Integration

Read `claude-instructions.md` and thoroughly inspect the existing EAEH implementation before making changes.

Implement the following enhancements so that users can operate EAEH through the Governance Portal without depending on CLI commands for routine project and workflow operations.

## 1. Project Onboarding Using an Existing Project Location

Allow users to initiate a new EAEH project by providing an existing local project directory.

The portal should provide a **Register Existing Project** workflow.

Required capabilities:

- Select or enter the existing project directory.
- Validate that the directory exists and is accessible.
- Detect the project structure, programming language, framework, build tool and Git repository where possible.
- Identify existing project documentation, including BRD, architecture documents, README, coding standards and project knowledge.
- Allow users to review and confirm detected project information before registration.
- Allow users to provide or amend project name, description, constraints, technology stack, BRD, knowledge sources, developer, steward and approver assignments.
- Associate the project with the correct EAEH workspace and tenant.
- Register the project using existing EAEH project registration services.
- Display the registered project immediately in the Portal.
- Provide clear validation errors for inaccessible paths, invalid project structures and duplicate registrations.

Do not copy or move the user's existing source code unnecessarily. Register and associate the existing workspace safely.

Support local development first. Do not expose arbitrary server filesystem access through browser requests. Use a controlled backend directory-selection or workspace-registration mechanism with path restrictions and authorization.

## 2. Start and Manage Workflows Through the Portal

Expose all currently implemented workflow activities and their supported operations in the Portal.

Users should be able to:

- Select a project.
- View workflows available for that project.
- Start a workflow.
- Select the appropriate workflow inputs and existing documents.
- Review the execution plan before starting.
- See the workflow progress and current stage.
- View individual task execution details.
- View agent assignments, dependencies and execution status.
- Inspect generated artifacts and validation results.
- Approve, reject or request changes wherever human approval is required.
- Cancel or retry workflows where supported.
- Resume workflows from their persisted state.

Do not hardcode workflow names or stages in the frontend. Retrieve supported workflow definitions and capabilities from EAEH backend services.

## 3. Resume or Reinitiate a Workflow From Any Supported Point

This is a critical requirement.

Provide a workflow recovery and restart capability.

Users should be able to select an existing workflow and choose a supported stage or task from which execution should continue.

Support the following scenarios:

- Resume a workflow waiting for approval.
- Resume a workflow waiting for external AI execution.
- Resume after a worker interruption or application restart.
- Retry a failed task.
- Re-execute a selected task.
- Restart from a selected workflow stage.
- Re-run a workflow from an earlier stage when requirements or architecture change.
- Resume downstream execution after a human approval.
- Reinitiate a completed workflow when a new version of its input documents is provided.

Before executing a restart or reinitiation:

1. Display the selected restart point.
2. Show affected downstream tasks.
3. Identify existing artifacts and their versions.
4. Explain whether existing outputs will be reused, invalidated or regenerated.
5. Identify approval decisions that remain valid and those that must be repeated.
6. Require user confirmation.

Important governance rules:

- Never silently overwrite previously approved artifacts.
- Preserve historical workflow executions and artifact versions.
- Maintain complete traceability between original and restarted executions.
- Revalidate downstream artifacts when upstream inputs change.
- Prevent duplicate task execution through idempotency controls.
- Respect dependencies, authorization, approval gates and execution policies.
- Do not allow users to bypass mandatory approval stages by selecting a later restart point.
- Do not allow arbitrary stage injection or execution outside registered workflow definitions.

Implement this through the existing workflow engine and persistence model. Do not introduce a second workflow engine in the Portal.

## 4. Complete CLI-to-Portal Feature Parity

Conduct a systematic inventory of all existing EAEH CLI commands and their corresponding backend services.

Every supported operational command should have an equivalent Portal capability wherever it is appropriate for a graphical interface.

Cover at least these functional areas:

| Functional area | Portal capabilities |
|---|---|
| Projects | Create, register, list, inspect and manage projects |
| Workspaces | Associate and inspect project workspaces |
| Workflows | List, start, inspect, resume, restart and cancel |
| Tasks | Inspect, retry and re-execute supported tasks |
| Requirements | Generate, inspect, version and approve |
| HLD | Generate, inspect, version and approve |
| LLD | Generate, inspect, version and approve |
| Skills | Discover, inspect, validate, approve and reject |
| Knowledge | Inspect registered knowledge and sources |
| Agents | Inspect registered agents and capabilities |
| Approvals | List, review, approve, reject and request changes |
| Artifacts | Browse, inspect, export and track versions |
| Execution | Inspect workers, execution state, failures and logs |
| Governance | View policy validation, audit history and decisions |
| Integrations | Inspect configured external AI and MCP integration status |

This table is a minimum coverage target. Derive the complete list from the actual CLI implementation.

For every command, classify it as:

- Portal-supported action.
- Portal read-only capability.
- CLI-only administrative or developer operation.
- Not applicable to the Portal.

Document why any command is intentionally excluded.

Do not create fake or non-functional UI controls for commands that have no backend implementation.

## 5. Unified Workflow and Task Dashboard

Enhance the dashboard to show the actual operational state across all registered projects.

Display:

- Total projects.
- Active workflows.
- Running tasks.
- Tasks waiting for external AI.
- Pending approvals.
- Failed or blocked workflows.
- Completed workflows.
- Recently executed tasks.

Provide filters by project, workflow, stage, task status and date.

For each workflow show:

- Project name.
- Workflow identifier.
- Workflow type.
- Current stage.
- Current task.
- Assigned agent.
- Execution status.
- Start and last-updated timestamps.
- Pending action or blocker.
- Available operations.

Users must be able to navigate from dashboard → project → workflow → task → artifact.

Use actual backend data, not hardcoded statistics or mock workflow states.

## 6. Workflow and Task Detail Experience

Provide detailed views for individual workflows and tasks.

Workflow view:

- Complete stage lifecycle.
- Stage dependencies.
- Completed and pending stages.
- Current execution state.
- Approval history.
- Generated artifacts.
- Task execution history.
- Failure details and recovery options.

Task view:

- Task description and input.
- Assigned agent and skill versions.
- Dependencies.
- Execution attempts.
- External AI handoff status where applicable.
- Output and generated artifacts.
- Validation results.
- Error details and logs.
- Retry and re-execution actions where supported.

Ensure the Portal reflects the actual persisted workflow and task state.

## 7. Backend and API Integration

Inspect the existing Rust services, CLI command handlers, workflow engine, persistence layer and MCP integration.

Reuse existing business logic and domain services.

Where Portal APIs are missing:

- Expose appropriate APIs through the existing backend architecture.
- Avoid duplicating CLI business logic in API handlers.
- Ensure CLI and Portal invoke the same underlying services.
- Apply consistent validation, authorization and governance checks.
- Ensure state changes are persisted and auditable.
- Provide consistent error responses.
- Add pagination and filtering for large result sets.
- Support appropriate real-time updates through existing event mechanisms or controlled polling.

The Portal must not directly modify database records to perform workflow operations.

## 8. External AI Execution Compatibility

Preserve EAEH's external AI execution architecture.

Claude Code and GitHub Copilot must continue to perform AI-intensive work through their supported integrations and existing subscriptions.

EAEH remains responsible for:

- Preparing execution tasks and context.
- Dispatching or exposing tasks through supported integration mechanisms.
- Receiving results.
- Validating returned artifacts.
- Enforcing approvals.
- Managing workflow progression.
- Maintaining audit and traceability.

Do not introduce direct Anthropic API calls, additional API billing, or hidden LLM fallback mechanisms.

Workflow recovery must correctly handle tasks waiting for external AI execution.

## 9. Security and Governance

Preserve existing EAEH security and governance controls.

- Do not bypass approval enforcement.
- Do not introduce privilege escalation.
- Enforce project and tenant isolation.
- Validate all filesystem paths.
- Prevent unauthorized workspace access.
- Treat external AI output as untrusted.
- Protect credentials and execution logs.
- Audit all workflow and task state changes.
- Require appropriate authorization for destructive or administrative operations.

The initial local-development Portal may retain its existing no-login experience, but do not weaken backend authorization or present it as production-ready.

## 10. Testing and Acceptance Criteria

Add backend, frontend, integration and end-to-end tests.

Mandatory scenarios:

1. Register an existing Spring Boot project by providing its location.
2. Discover its project structure and existing documentation.
3. Start a requirements/HLD/LLD design workflow through the Portal.
4. View every stage and task from the Portal.
5. Approve requirements and verify workflow progression.
6. Approve HLD and verify LLD generation.
7. Resume a workflow waiting for external AI execution.
8. Recover a workflow after worker interruption.
9. Retry a failed task.
10. Restart execution from an earlier supported stage.
11. Change an upstream artifact and verify downstream invalidation.
12. Verify approved artifacts are not silently overwritten.
13. Verify CLI and Portal display consistent workflow state.
14. Verify all supported CLI operations are represented in the Portal inventory.
15. Verify authorization, audit history and project isolation.

Use the existing `simple-springboot-api-b` project as an end-to-end test candidate. Do not overwrite its active workflow, approvals or artifacts. Use isolated test data when required.

## 11. Documentation

Update the relevant existing documentation and user guides.

Document:

- Registering an existing project by location.
- Starting workflows from the Portal.
- Workflow and task monitoring.
- Resuming and restarting workflows.
- Retry and recovery behaviour.
- CLI-to-Portal feature parity.
- Limitations and CLI-only operations.
- Security and local-development restrictions.

Add an implementation document containing the Portal capability inventory and test evidence.

## 12. Implementation Constraints

- Read `claude-instructions.md` before implementation.
- Inspect the current architecture and implementation before changing anything.
- Implement only within Phases 1–3.
- Do not modify, reorder, rename or reinterpret the eight-phase roadmap.
- Do not change any existing next steps, phase dependencies, acceptance criteria or implementation instructions in `claude-instructions.md`.
- Do not start implementation of Phases 4–8.
- Preserve existing functionality and backward compatibility.
- Prefer extending existing services over introducing new subsystems.
- Do not add a separate database or workflow engine.
- Do not commit or push changes.

## 13. Final Delivery Report

Provide an evidence-based report containing:

1. Features implemented.
2. Project onboarding flow.
3. Workflow resume/restart capabilities.
4. CLI-to-Portal feature parity matrix.
5. Backend APIs added or reused.
6. Frontend screens and interactions added.
7. Files changed.
8. Database/schema changes, if any.
9. Automated test results.
10. End-to-end test evidence.
11. Known limitations and unsupported CLI operations.
12. Documentation updated.
13. Remaining gaps and recommended next actions.

Do not report a feature as implemented unless its backend operation, Portal interaction and relevant tests are verified.

Stop after completing this scope and wait for my approval before undertaking any further phase or roadmap work.