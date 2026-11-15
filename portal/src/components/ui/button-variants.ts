const base =
  "inline-flex h-9 items-center justify-center gap-2 whitespace-nowrap rounded-md px-4 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50";

const variants = {
  default: "bg-primary text-primary-foreground hover:bg-primary/90",
  destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
  success: "bg-emerald-700 text-white hover:bg-emerald-700/90",
  warning: "bg-amber-600 text-white hover:bg-amber-600/90",
  outline: "border border-input bg-card hover:bg-accent",
} as const;

export function buttonVariantsFor(v: keyof typeof variants): string {
  return `${base} ${variants[v]}`;
}
