# Error catalog

| Code | Use when |
|---|---|
| `VALIDATION_FAILED` | The request body or parameters are invalid (list every field). |
| `RESOURCE_NOT_FOUND` | The resource does not exist or is not visible to the caller. |
| `CONFLICT` | The request conflicts with the current state (duplicate, version mismatch). |
| `RATE_LIMITED` | The caller exceeded a limit; include `retry_after_seconds`. |
| `INTERNAL_ERROR` | Anything unexpected; log details server-side only. |
