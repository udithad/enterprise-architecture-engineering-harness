# Error response format (DEMONSTRATION STANDARD)

Every HTTP API error response MUST use the media type `application/problem+json`.

## Required fields

Each error body MUST contain `type`, `title`, `status` and `trace_id`.

## Status codes

Client errors use 4xx status codes; a 500 response must never be used for validation failures.
