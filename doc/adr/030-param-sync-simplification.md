# ADR-030: Param Sync Simplification

## Context

Runtime parameter synchronization uses a two-stage atomic pattern:

```cpp
_param_{name}_read.store(_param_{name}_write.load(std::memory_order_acquire), std::memory_order_release);
{type} _param_{name}_val = _param_{name}_read.load(std::memory_order_acquire);
```

This was originally designed for a scenario where external readers might observe `_param_{name}_read`. However, grep confirms `_param_{name}_read` has zero external consumers in `runtime/` (`pipit.h`, `pipit_shell.h`, or any other runtime header). The intermediate `_read` atomic is dead storage.

## Decision

Collapse param sync to a single acquire load from `_param_{name}_write`:

```cpp
{type} _param_{name}_val = _param_{name}_write.load(std::memory_order_acquire);
```

Remove the `_param_{name}_read` atomic declaration entirely from `emit_param_storage()`.

## Consequences

- One fewer atomic operation per parameter per tick.
- Simpler generated code — easier to audit and debug.
- `_param_{name}_read` declaration removed, reducing generated code size.
- Single acquire load preserves the same visibility guarantee: the actor sees the most recent value written by the shell thread.

## Alternatives

- Keep `_param_read` for potential future external readers. Rejected: YAGNI — no consumer exists, and re-adding is trivial if needed.
- Use `relaxed` ordering instead of `acquire`. Rejected: `acquire` is needed to ensure the actor sees the value written by the shell's `release` store.

## Exit criteria

- `_param_{name}_read` declaration removed from generated C++.
- Param sync uses single `_param_{name}_write.load(std::memory_order_acquire)`.
- Param-using pipelines compile and run correctly.
- All existing tests pass unchanged.
