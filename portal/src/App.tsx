import { lazy, Suspense, type ReactNode } from "react";
import { createBrowserRouter, RouterProvider } from "react-router";
import { AppShell } from "@/components/layout/AppShell";
import { LoadingRows } from "@/components/States";
import { DashboardPage } from "@/pages/Dashboard";

// The dashboard loads eagerly; other screens (notably the Markdown document viewer) are
// split into their own chunks.
const page = <T extends Record<string, () => ReactNode>>(load: () => Promise<T>, name: keyof T) =>
  lazy(async () => ({ default: (await load())[name] as () => ReactNode }));
const ProjectsPage = page(() => import("@/pages/Projects"), "ProjectsPage");
const ProjectDetailPage = page(() => import("@/pages/ProjectDetail"), "ProjectDetailPage");
const WorkflowsPage = page(() => import("@/pages/Workflows"), "WorkflowsPage");
const WorkflowDetailPage = page(() => import("@/pages/WorkflowDetail"), "WorkflowDetailPage");
const ApprovalsPage = page(() => import("@/pages/Approvals"), "ApprovalsPage");
const ApprovalReviewPage = page(() => import("@/pages/ApprovalReview"), "ApprovalReviewPage");
const ActivityPage = page(() => import("@/pages/Activity"), "ActivityPage");
const SkillsPage = page(() => import("@/pages/Skills"), "SkillsPage");
const SkillViewerPage = page(() => import("@/pages/SkillViewer"), "SkillViewerPage");
const TasksPage = page(() => import("@/pages/Tasks"), "TasksPage");
const TaskDetailPage = page(() => import("@/pages/TaskDetail"), "TaskDetailPage");
const RegisterProjectPage = page(() => import("@/pages/RegisterProject"), "RegisterProjectPage");
const StartWorkflowPage = page(() => import("@/pages/StartWorkflow"), "StartWorkflowPage");
const ExecutionPage = page(() => import("@/pages/Execution"), "ExecutionPage");
const CatalogPage = page(() => import("@/pages/Catalog"), "CatalogPage");
const SettingsPage = page(() => import("@/pages/Settings"), "SettingsPage");
const NotFoundPage = page(() => import("@/pages/NotFound"), "NotFoundPage");

const s = (el: ReactNode) => <Suspense fallback={<LoadingRows rows={4} />}>{el}</Suspense>;

export const routes = [
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: "projects", element: s(<ProjectsPage />) },
      { path: "projects/register", element: s(<RegisterProjectPage />) },
      { path: "projects/:id", element: s(<ProjectDetailPage />) },
      { path: "projects/:id/start", element: s(<StartWorkflowPage />) },
      { path: "tasks", element: s(<TasksPage />) },
      { path: "tasks/:id", element: s(<TaskDetailPage />) },
      { path: "execution", element: s(<ExecutionPage />) },
      { path: "catalog", element: s(<CatalogPage />) },
      { path: "workflows", element: s(<WorkflowsPage />) },
      { path: "workflows/:id", element: s(<WorkflowDetailPage />) },
      { path: "approvals", element: s(<ApprovalsPage />) },
      { path: "approvals/:id", element: s(<ApprovalReviewPage />) },
      { path: "activity", element: s(<ActivityPage />) },
      { path: "skills", element: s(<SkillsPage />) },
      { path: "skills/:id/:version", element: s(<SkillViewerPage />) },
      { path: "settings", element: s(<SettingsPage />) },
      { path: "*", element: s(<NotFoundPage />) },
    ],
  },
];

export function App() {
  return <RouterProvider router={createBrowserRouter(routes)} />;
}
