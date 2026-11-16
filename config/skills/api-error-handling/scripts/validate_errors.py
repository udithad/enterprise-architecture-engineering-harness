"""Reviewer helper: check that an LLD JSON lists error responses for every operation.

Shown in the portal for review only. EAEH never executes skill scripts or sends them to agents.
"""
import json
import sys


def main(path: str) -> int:
    lld = json.load(open(path, encoding="utf-8"))
    missing = [op.get("id") for c in lld.get("components", []) for op in c.get("operations", []) if not op.get("errors")]
    for m in missing:
        print(f"operation without error responses: {m}")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
