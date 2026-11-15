import { Link } from "react-router";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/States";

export function NotFoundPage() {
  return (
    <EmptyState title="Page not found">
      <Button asChild size="sm" variant="outline" className="mt-2">
        <Link to="/">Back to dashboard</Link>
      </Button>
    </EmptyState>
  );
}
