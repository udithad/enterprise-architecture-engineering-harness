+++
# EAEH skill directory. This TOML front matter is the manifest; the Markdown below is the
# main instruction text. Every file in this folder is covered by the approval (tree hash).
id = "api-error-handling"
version = "1.0.0"
description = "Consistent API error responses and HTTP status mapping in the LLD."
task_kinds = ["lld-design"]
max_risk = "read_only"
evaluation_criteria = [
  "every API operation lists its error responses",
  "error codes come from the error catalog",
]
+++

# API error handling

Design each API operation's error behaviour explicitly. Apply the reference files of this skill:

- `references/error-catalog.md`: the only allowed error codes and when to use each.
- `references/http/status-mapping.md`: which HTTP status goes with which situation.
- `examples/error-response.json`: the response body shape.
- `examples/config.yaml`: default limits to state in the design.

Rules:
1. Every operation lists its possible error responses: status, error code, and when it occurs.
2. Use the response body shape from the example. Never return stack traces or internal identifiers.
3. Validation errors list every invalid field, not just the first.
4. Unknown resources return 404 with `RESOURCE_NOT_FOUND`, the same answer whether the resource is missing or not visible to the caller.
