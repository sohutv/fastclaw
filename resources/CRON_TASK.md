# CRON_TASK.md — Scheduled Task Execution Specification

## 1. Basic Task Types
### 1.1 Stateless Scheduled Tasks
Each execution of the task is logically independent and does not require recording execution status, for example: regular reminders to drink water, regular reminders to stand up and move around, etc.
- Execution Rule: Trigger on schedule, no additional status update required.

### 1.2 Stateful Scheduled Tasks
Task execution depends on the state of the previous execution, for example: increment counting tasks, cumulative counting tasks, etc.
- Execution Rules:
  1. Before each execution, first read the current state from the task description
  2. Execute business logic and update the state
  3. After execution, the **latest state must be updated back to the task description** to ensure correct state for the next execution

### 1.3 Scheduled Tasks with Termination Conditions
Tasks have clear termination conditions, and do not need to continue triggering after the conditions are met.
- Execution Rules:
  1. After each execution, automatically perform task self-inspection
  2. Check whether the termination condition is met
  3. If the condition is met, automatically delete/disable the task to terminate subsequent scheduling
  4. If not met, keep the task active and wait for the next trigger

## 2. Standard Execution Flow
```
1. Task Triggered → 2. Get Task Details → 3. Read Current State (if any)
    ↓
4. Execute Business Logic → 5. Update Task State to Task Description (for stateful tasks)
    ↓
6. Check whether termination condition is met (for tasks with termination conditions)
    ↓
    ├─ Condition Met → Delete/Disable Task → Exit
    └─ Condition Not Met → Keep Task Active → Wait for Next Trigger
```

## 3. Special Notes
- All state changes must be persisted to the task description, do not rely on session memory, ensure consistent state across sessions
- Termination condition check must be performed after each execution to avoid invalid tasks continuing to occupy scheduling resources
