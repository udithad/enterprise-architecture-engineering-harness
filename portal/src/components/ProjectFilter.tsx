import { useProjects } from "@/hooks/queries";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

export function ProjectFilter({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const projects = useProjects();
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="w-52" aria-label="Filter by project">
        <SelectValue placeholder="All projects" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="all">All projects</SelectItem>
        {projects.data?.items.map((p) => (
          <SelectItem key={p.id} value={p.id}>
            {p.name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
