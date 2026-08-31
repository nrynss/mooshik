---
title: Adversarial Reviews
description: Multi-stage adversarial review discipline and milestone validation records.
---

Mooshik development follows a strict adversarial review discipline. Every milestone is evaluated against threat models and failure cases before approval.

## The Review Process

Each architectural milestone undergoes multiple review rounds:

1. **Implementation audit:** Validates code against specifications and security boundaries.
2. **Adversarial stress testing:** Probes edge cases, including permission bypass attempts, concurrent write races, unhandled timeouts, and secret leakage paths.
3. **Remediation cycles:** Identified issues are remediated and re-audited until achieving approval with zero residue.

## Milestone Review Records

Detailed review records reside in `dev-diary/adversarial-review/`:

- **M1 to M3:** Store schema provisioning, vault file permissions, and environment variable overlay validation.
- **M4 to M7:** Companion adapter streaming, permissions boundary enforcement, and tool egress redaction.
- **M8 to M9:** Bootstrap ingester scaling, Cloud Run deployment, and M9 measurement calibration.
- **M10:** MCP client host framing safety and vault secret injection.
- **M12a to M12h:** TUI live tick duration pins, workspace watcher filtering, reflection merging idempotence, and guided setup secret masking.
