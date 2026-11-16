# HTTP status mapping

| Situation | Status | Code |
|---|---|---|
| Invalid input | 400 | `VALIDATION_FAILED` |
| Missing or invisible resource | 404 | `RESOURCE_NOT_FOUND` |
| State conflict | 409 | `CONFLICT` |
| Too many requests | 429 | `RATE_LIMITED` |
| Unexpected failure | 500 | `INTERNAL_ERROR` |
