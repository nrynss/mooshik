---
title: Adversarial Review Protocol
description: Multi-agent review and remediation workflow for milestone validation.
---

Every milestone in Mooshik undergoes structured adversarial review rounds before merge.

## The Review Cycle

The process follows a three-stage loop:

```
[ Implementation Agent ]
           |
           v
   [ Review Agent ] <------+
           |               |
       (Findings?)         |
        /       \          |
     (Yes)      (No)       |
      /           \        |
     v             v       |
[ Remediator ]   [ APPROVE ]
     |                     |
     +---------------------+
```

### 1. Implementation
The engineer implements the feature in an isolated git worktree and produces an initial report.

### 2. Adversarial Review
The reviewer runs all quality gates independently. It checks claims against actual code and files findings by priority:
- **P1**: Security vulnerabilities, missing core behaviors, and specification violations.
- **P2**: Gate failures, lint warnings, and missing test coverage.
- **P3**: Documentation and style issues.

### 3. Remediation
The remediator resolves every finding across all priority levels. The review loop repeats until the reviewer awards an explicit approval with zero critical residue.
