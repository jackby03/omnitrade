# omnitrade — Agent Orchestration Backlog

## How This System Works

This directory contains the **Engineering Task Description (EDT)** backlog for the omnitrade project. Each `.md` file is a single, atomic ticket designed to be assigned to a small coding agent.

### Orchestration Model

```
Orchestrator Agent
    ├── reads backlog/*.md to find TODO tickets
    ├── resolves dependencies (no ticket runs before its deps are DONE)
    ├── assigns ticket to a Worker Agent with the embedded prompt
    └── marks ticket as DONE upon successful verification
```

### Ticket Lifecycle

```
TODO  →  IN_PROGRESS  →  REVIEW  →  DONE
                │                      
                └──── BLOCKED (dependency not met)
```

Update the `Status` field in the ticket's metadata table to track progress.

### Rules for Worker Agents

1. **Read `.agents/AGENTS.md`** first for project-wide coding standards.
2. **Read your assigned ticket file** for task-specific requirements.
3. **Only modify files listed in your ticket's `Target` field.**
4. **Maximum output**: 250 lines of production code per file.
5. **Must include** inline `#[cfg(test)] mod tests` with Arrange-Act-Assert.
6. **Zero** `panic!`, `.unwrap()`, `.expect()`, or `unsafe` in production code.
7. **Return** `Result<T, E>` for all fallible operations.

### Dependency Resolution

Each ticket has a `Depends On` field listing prerequisite ticket IDs. The orchestrator MUST NOT assign a ticket until ALL dependencies have status `DONE`.

Tickets with no dependencies (or all deps `DONE`) can be executed **in parallel**.

### Parallel Execution Groups

Within each phase, tickets are ordered by dependency. Independent tickets can run simultaneously:

| Parallel Group | Tickets |
|---|---|
| Phase 2A | `EXCH-001` |
| Phase 2B | `EXCH-002`, `EXCH-003` (after 2A) |
| Phase 2C | `EXCH-004` (after 2B) |
| Phase 2D | `EXCH-005` (after 2C) |
| Phase 3A | `ENG-001` |
| Phase 3B | `ENG-002`, `ENG-003` (after 3A, parallel) |
| Phase 3C | `ENG-004` (after 3B) |
| Phase 3D | `ENG-005` (after 3A + 3C) |
| Phase 4A | `SCR-001` |
| Phase 4B | `SCR-002` (after 4A) |
| Phase 4C | `SCR-003` (after 4B) |
| Phase 4D | `SCR-004` (after 4C) |
| Phase 4E | `SCR-005` (after 4D) |
| Phase 4F | `CLI-001` (after 3D) |
| Phase 4G | `CLI-002` (after 4F + 4E) |
| Phase 5A | `UI-001` (after 3D) |
| Phase 5B | `UI-002`, `UI-003` (after 5A, parallel) |
| Phase 5C | `UI-004` (after 5B) |

### Dependency DAG

```
CORE-* (DONE)
    │
    ├──► EXCH-001 ──► EXCH-002 ──┐
    │                 EXCH-003 ──┤
    │                            ▼
    │                      EXCH-004 ──► EXCH-005
    │
    ├──► ENG-001 ──► ENG-002 ──┐
    │              ► ENG-003 ──┤
    │                          ▼
    │                    ENG-004 ──► ENG-005
    │
    ├──► SCR-001 ──► SCR-002 ──► SCR-003 ──► SCR-004 ──► SCR-005
    │
    ├──► CLI-001 (needs ENG-005) ──► CLI-002 (needs SCR-005)
    │
    └──► UI-001 (needs ENG-005) ──► UI-002 ──┐
                                    UI-003 ──┤
                                             ▼
                                       UI-004
```

### Ticket File Format

Every ticket uses the same structure:

```
# [ID]: [Title]

| Field | Value |
|---|---|
| Status | TODO / IN_PROGRESS / REVIEW / DONE / BLOCKED |
| Phase | 2-5 |
| Crate | omnitrade-xxx |
| Target | src/path.rs |
| Depends On | TICKET-IDs |
| Blocks | TICKET-IDs |
| Complexity | S (< 100 LOC) / M (100-200 LOC) / L (200-250 LOC) |

## Description
## Requirements
## Acceptance Criteria
## Cargo Dependencies
```
