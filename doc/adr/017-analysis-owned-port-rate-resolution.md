# ADR-017: Analysis-Owned Node Port-Rate Resolution

## Context

From v0.2.x through early v0.3.1, concrete port-rate resolution logic existed in three phases:

1. `analyze` (SDF balance, shape propagation, diagnostics)
2. `schedule` (edge buffer sizing)
3. `codegen` (stride/index generation for actor firing)

All three phases resolved symbolic token counts from partially overlapping sources (explicit args, shape constraints, span-derived dimensions, and inferred shapes). This duplication introduced semantic drift risk: schedule/codegen could diverge from analysis when inference precedence changed or new shape cases were added.

This risk conflicted with the backend contract in `pcc-spec-v0.3.0`: downstream phases should be syntax-directed consumers of resolved semantics, not independent inference engines.

## Decision

### 1. Make `analyze` the single source of truth for concrete node rates

- Add `node_port_rates: HashMap<NodeId, NodePortRates>` to `AnalyzedProgram`.
- `NodePortRates` stores `{ in_rate: Option<u32>, out_rate: Option<u32> }`.
- Compute these rates once in analysis after shape/span resolution and expose them downstream.

### 2. Remove duplicate rate inference from `schedule`

- `schedule` no longer resolves actor `PortShape` on its own.
- Edge buffer sizing uses `analysis.node_port_rates[node].out_rate`.
- If unavailable, preserve prior safe fallback (`1`).

### 3. Remove duplicate rate inference from `codegen`

- `codegen` no longer re-resolves in/out port rates from actor metadata.
- Actor firing stride selection uses `analysis.node_port_rates` first.
- Existing safe fallbacks remain for unresolved cases (e.g., `edge_tokens / repetition`).

### 4. Keep current dimension-parameter fallback behavior as a separate concern

- This ADR covers concrete per-node port-rate ownership.
- Remaining dimension-parameter fallback duplication is explicitly deferred.

## Consequences

- **Consistency improvement**: schedule/codegen consume identical concrete rates from analysis.
- **Lower maintenance cost**: one inference implementation instead of three.
- **Reduced regression risk**: precedence changes in dimension inference no longer require synchronized edits across phases.
- **Minor cost**: small extra analysis result footprint for cached node rates.
- **Known follow-up**: dimension-parameter materialization still has partial duplication outside rate ownership.

## Alternatives considered

- **Keep three independent resolvers**: rejected due to repeated drift/regression risk.
- **Factor shared helper but keep per-phase inference**: rejected; code sharing reduces duplication but not phase ownership ambiguity.
- **Move authority to `schedule`**: rejected because analysis is the earliest phase with full shape diagnostics and inferred-shape state.
- **Infer only in `codegen` from scheduled buffers**: rejected; over-couples codegen to schedule artifacts and weakens analysis-to-backend contract.

## Exit criteria

- [x] `AnalyzedProgram` exposes `node_port_rates`.
- [x] `schedule` uses analysis-owned out-rates for edge buffer sizing.
- [x] `codegen` uses analysis-owned in/out rates for stride decisions.
- [x] Legacy duplicate rate resolvers removed from `schedule`/`codegen`.
- [x] Compiler and integration tests pass after refactor.
- [x] Runtime performance check shows no clear regression beyond measurement noise.
