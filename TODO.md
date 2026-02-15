# TODO

## Stage 2: Auditor Session Safety (Planned)

- Add stale-context detection/reset for persistent auditor sessions.
- Reset an auditor session when its task definition or relevant task-tree structure changes materially.
- Log when an auditor session is reused vs reset for traceability.
- Add regression tests covering session invalidation across task edits, resume flows, and task deletion/reparenting.
