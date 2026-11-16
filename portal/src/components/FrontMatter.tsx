/** The complete YAML front matter of a community SKILL.md, as written (all properties). */
export function FrontMatter({ value }: { value: Record<string, unknown> }) {
  const entries = Object.entries(value).filter(([k]) => k !== "description");
  return (
    <dl className="grid grid-cols-1 gap-x-6 gap-y-2 text-sm md:grid-cols-2" data-testid="frontmatter">
      {entries.map(([k, v]) => (
        <div key={k} className="min-w-0">
          <dt className="font-mono text-xs text-muted-foreground">{k}</dt>
          <dd className="break-words">
            {v !== null && typeof v === "object" ? (
              Array.isArray(v) ? (
                v.map(String).join(", ")
              ) : (
                <dl className="mt-1 border-l pl-3">
                  {Object.entries(v as Record<string, unknown>).map(([k2, v2]) => (
                    <div key={k2} className="flex gap-2 text-xs">
                      <dt className="shrink-0 font-mono text-muted-foreground">{k2}</dt>
                      <dd className="break-words">{v2 !== null && typeof v2 === "object" ? JSON.stringify(v2) : String(v2)}</dd>
                    </div>
                  ))}
                </dl>
              )
            ) : (
              String(v)
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
