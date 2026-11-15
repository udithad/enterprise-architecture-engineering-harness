# EAEH Governance Portal – Professional UI Implementation Amendment

Read the existing `claude-instructions.md` and the current EAEH implementation carefully before making any changes.

## 1. Critical instructions – preserve the existing roadmap

**Do not modify, reorder, remove, rename, reinterpret or change any existing next steps, phase definitions, phase dependencies, acceptance criteria or implementation instructions in `claude-instructions.md`.**

In particular:

- Preserve all eight existing phases exactly as currently defined.
- Do not change the approved architecture or previously agreed technical decisions.
- Do not move functionality from one phase to another.
- Do not introduce new mandatory dependencies into later phases.
- Do not change the existing phase completion or approval process.
- Do not start or implement Phase 4 or any subsequent phase as part of this UI enhancement.

This task is an additional UI enhancement for the existing implementation roadmap, not a replacement for it.

If documentation needs updating, add a clearly separated amendment or supplementary section. Preserve all existing instructions and content.

## 2. Objective

Implement a professional, enterprise-grade **EAEH Governance Portal** that provides a user-friendly web interface for monitoring engineering workflows and reviewing human approval requests.

The portal must integrate with the existing EAEH Rust backend and governance services.

It must not introduce a separate approval engine or duplicate existing workflow logic.

### Technology stack

Frontend:
- React 19
- TypeScript
- Vite
- Tailwind CSS 4
- shadcn/ui with accessible Radix UI components
- React Router
- TanStack Query
- Zustand only where shared client-side state is necessary
- React Hook Form and Zod where forms are required
- Lucide icons

Backend:
- Existing EAEH Rust implementation.
- Axum for HTTP APIs if consistent with the existing backend architecture.
- Reuse existing workflow, approval, governance and authorization services.
- Reuse the existing database and persistence implementation.
- Do not introduce a separate database for the portal.
- Do not migrate the database or replace existing persistence as part of this enhancement.

Use compatible, stable dependency versions and pin them appropriately.

## 3. Authentication scope – initial implementation

For this initial portal implementation, **do not implement authentication**.

Requirements:
- No login page.
- No registration page.
- No password management.
- No OAuth/OIDC integration.
- No Keycloak integration.
- No authentication-provider setup.
- No new user-management subsystem.

The portal should open directly into its dashboard when accessed locally.

Important:
- This is an initial local-development experience, not a production security model.
- Do not expose the portal as an unauthenticated production or multi-tenant application.
- Preserve existing backend authorization and approval enforcement.
- Never bypass approval authorization simply because the frontend does not have a login page.
- Clearly document that authentication must be introduced before exposing the portal beyond the trusted local development environment.
- Do not weaken or remove existing backend security controls.

## 4. Professional dashboard – highest priority

When the user opens the portal, the first screen must be a polished dashboard.

The dashboard must provide immediate visibility into the current state of EAEH engineering work.

### Dashboard sections

**A. Overview cards**

Display:
- Total projects.
- Workflows in progress.
- Tasks currently executing.
- Pending approvals.
- Completed workflows.
- Failed or blocked workflows.

Values must come from actual backend data. Do not use fabricated statistics.

**B. In-progress engineering tasks**

This is a key requirement.

Display tasks and workflows currently in progress, including:

- Project name.
- Workflow name and ID.
- Current lifecycle stage.
- Task name.
- Assigned agent.
- Execution status.
- Start time.
- Last updated time.
- Duration, where available.
- Current progress or stage information, where available.
- Whether the workflow is waiting for external AI execution, human approval, or system processing.

Provide clear visual status indicators:
- Running.
- Waiting for AI.
- Waiting for approval.
- Completed.
- Failed.
- Cancelled.

Users must be able to select an in-progress item and navigate to its detailed workflow view.

Refresh dashboard data automatically using an appropriate polling strategy or existing event infrastructure. Avoid excessive backend requests.

**C. Pending approvals**

Show all pending approvals across projects.

For each approval display:
- Project.
- Workflow.
- Document type (Requirements, HLD, LLD).
- Approval status.
- Created date.
- Associated artifact version.
- Approver role.

Provide a direct action to open the document review screen.

**D. Recent activity**

Display relevant recent events:
- Workflow started.
- AI task completed.
- Artifact generated.
- Approval requested.
- Approval granted.
- Changes requested.
- Workflow failed or resumed.

Use actual audit and workflow events.

## 5. Approval management module

Implement a dedicated approval experience for the existing workflow approval lifecycle.

Initially support:
- Requirements approval.
- HLD approval.
- LLD approval.

### Approval document viewer

The reviewer must be able to:
- Open and read the full document.
- View document metadata.
- See project and workflow information.
- Identify artifact version and hash.
- Navigate between related requirements and architecture artifacts where traceability data exists.
- See validation and policy results where available.

### Approval actions

Provide three clear actions:

1. Approve.
2. Request changes.
3. Reject.

Requirements:
- Capture reviewer comments.
- Require comments for request-changes and rejection decisions.
- Display an explicit confirmation before submitting a decision.
- Submit decisions through the existing EAEH approval service.
- Preserve existing approver-role and authorization checks.
- Use the artifact hash/version expected by the backend.
- Prevent stale-document approval.
- Display clear success and failure feedback.

The UI must never directly update workflow database records.

The backend remains authoritative for every approval decision.

After a successful approval, the existing workflow engine must resume the workflow according to its existing behavior.

## 6. Workflow detail screen

Provide a visual lifecycle view for each workflow.

For the existing design workflow, display:

BRD → Requirements Analysis → Requirements Review → Requirements Approval → HLD Generation → HLD Approval → LLD Generation → LLD Approval

Show:
- Completed stages.
- Active stage.
- Pending approval gates.
- Failed stages.
- Execution timestamps.
- Associated artifacts.
- Task and agent information.

The display must be generated from actual workflow definitions and execution records, not hardcoded assumptions about all possible workflows.

Allow users to navigate from a stage to its associated task, document or approval.

## 7. Project management view

Provide a project listing and project details view.

Display:
- Project name.
- Description.
- Project status.
- Active workflows.
- Pending approvals.
- Recent activity.
- Available engineering artifacts.

Allow users to navigate from a project to its workflows and approvals.

Do not implement new project-creation or project-configuration functionality unless the existing APIs already support it and it is required for the current portal experience.

## 8. User experience and visual design requirements

The portal must look like a professional enterprise software product.

Design expectations:
- Clean and modern enterprise dashboard.
- Consistent typography and spacing.
- Professional neutral color palette.
- Clear information hierarchy.
- Well-designed navigation sidebar.
- Responsive layouts for desktop and tablet.
- Accessible components and keyboard navigation.
- Meaningful loading, empty and error states.
- Clear confirmation dialogs for important actions.
- Consistent status indicators.
- Useful tables, filters and search.
- Avoid excessive gradients, decorative animations and unnecessary visual clutter.

The portal should feel comparable in quality to established enterprise engineering and governance platforms.

Suggested navigation:

- Dashboard
- Projects
- Workflows
- Approvals
- Activity / Audit
- Settings (only existing applicable settings; do not create unnecessary configuration features)

Do not create decorative controls that do not work.

Every visible button, filter, navigation item and action must have a functioning implementation.

## 9. Backend API integration

Before creating APIs, inspect the existing EAEH backend.

Reuse existing services wherever possible.

Implement only the API endpoints required by the portal and follow existing EAEH conventions.

Potential API capabilities include:

- Dashboard summary.
- Project listing and details.
- Active workflow listing.
- Workflow details and stage history.
- Task execution status.
- Pending approval listing.
- Approval document retrieval.
- Approval decision submission.
- Recent activity and audit events.

These are capability requirements, not instructions to create duplicate endpoints if equivalent functionality already exists.

Requirements:
- Validate all incoming requests.
- Enforce backend authorization and governance.
- Use consistent error responses.
- Avoid exposing internal credentials or sensitive configuration.
- Document APIs using the existing API documentation approach.
- Keep frontend and backend responsibilities clearly separated.

## 10. Scope restriction – phases 1 to 3 only

This UI enhancement must affect only the work and capabilities applicable to the existing roadmap through Phase 3.

The portal may provide visibility and interaction for:
- Phase 1: Core Harness Foundation.
- Phase 2: Enterprise Knowledge and Skills Management.
- Phase 3: Requirements Engineering and Architecture Generation.

Do not implement or change:
- Phase 4 multi-agent software development functionality.
- Phase 5 testing and quality engineering functionality.
- Phase 6 Git and CI/CD functionality.
- Phase 7 release and deployment management functionality.
- Phase 8 enterprise scale and advanced capabilities.

Do not change their existing roadmap definitions, dependencies, acceptance criteria or next steps.

If a dashboard requires displaying future-phase capabilities, show only existing available data or clearly indicate that the capability is not yet available. Do not implement the future capability.

## 11. Testing and acceptance criteria

Add appropriate frontend, backend and integration tests.

At minimum, verify:

1. Portal starts successfully in local development.
2. Dashboard opens without a login screen.
3. Dashboard displays real EAEH project and workflow data.
4. In-progress tasks are visible.
5. Waiting-for-AI and waiting-for-approval states are distinguishable.
6. Pending requirements, HLD and LLD approvals are displayed.
7. Documents can be opened and read.
8. Approve, request-changes and reject actions work through the existing approval service.
9. Approval decisions preserve artifact hashes and audit records.
10. Workflow progression occurs through the existing workflow engine.
11. Failed API requests produce useful error messages.
12. Empty states are handled correctly.
13. No future-phase functionality has been implemented.
14. Existing CLI, MCP and workflow functionality continues to work.

Use actual EAEH data for end-to-end testing. Do not claim success based only on mocked frontend data.

## 12. Documentation

Create a supplementary document:

`docs/implementation/EAEH-GOVERNANCE-PORTAL.md`

Document:
- Architecture.
- Technology stack.
- Frontend structure.
- Backend integration.
- API contracts.
- Dashboard functionality.
- Approval lifecycle.
- Local startup instructions.
- Testing instructions.
- Authentication limitation and security implications.
- Scope limitations through Phase 3.
- Future considerations.

Update existing user guides only where necessary to explain how to use the new portal.

**Do not modify existing next-step instructions or phase roadmap content in `claude-instructions.md`.**

## 13. Implementation process

1. Read `claude-instructions.md` in full.
2. Inspect the existing repository, backend, workflow engine, approval services and persistence.
3. Identify existing reusable API capabilities.
4. Propose the portal architecture based on the actual implementation.
5. Implement the portal and required backend integration.
6. Add tests.
7. Run existing regression tests.
8. Verify the existing CLI and MCP integrations.
9. Verify that the implementation does not affect Phases 4–8.
10. Update supplementary documentation.
11. Produce an evidence-based implementation report.

Do not rewrite working components unnecessarily.

Do not introduce new microservices or a separate database.

Do not change approved architecture decisions without explicit authorization.

Do not commit or push changes.

Stop after completing this portal enhancement. Do not proceed to the next roadmap phase without explicit approval.

## 14. Final delivery report

Provide:
- Files created and modified.
- Frontend and backend architecture.
- Implemented screens and functionality.
- API endpoints reused or introduced.
- Database/persistence changes, if any.
- Authentication limitations.
- Test commands and actual results.
- Regression test results.
- Confirmation that existing roadmap next steps remain unchanged.
- Confirmation that implementation is limited to Phases 1–3.
- Outstanding issues and recommendations.

Do not claim completion for any feature that has not been implemented and tested.