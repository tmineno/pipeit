# v0.3.3 Profiling Notes

Date: 2026-02-20

## Goal

Capture compiler-hotspot evidence using runtime profiling, then prioritize optimization work from measured cost.

## Method

### 1) Stable benchmark sanity check

```bash
perf stat -r 3 -- taskset -c 1 cargo bench \
  --manifest-path compiler/Cargo.toml \
  --bench compiler_bench \
  -- "kpi/full_compile_latency/complex" \
  --sample-size 30 --measurement-time 0.8 --warm-up-time 0.2
```

### 2) Function-level profiler capture on real compile path

Criterion itself spends time in statistics/reporting internals, so hotspot capture was done on direct `pcc` compilation loop:

```bash
perf record -F 999 -g -o /tmp/pcc-compile-modal.perf -- taskset -c 1 bash -lc '
  for i in $(seq 1 30000); do
    ./target/release/pcc benches/pdl/modal.pdl \
      -I runtime/libpipit/include/std_actors.h \
      -I runtime/libpipit/include/std_math.h \
      -I examples/example_actors.h \
      --emit cpp -o /tmp/pcc_modal.cpp >/dev/null 2>&1
  done
'
perf report -i /tmp/pcc-compile-modal.perf --stdio --no-children --sort symbol
```

## Top Observed Hotspots (modal compile loop)

- `<pcc::registry::ActorMeta as core::clone::Clone>::clone` ~2.85%
- `core::ptr::drop_in_place<pcc::registry::ActorMeta>` ~2.67%
- `pcc::type_infer::TypeInferEngine::infer_from_pipe_context_with_initial` ~2.03%
- `pcc::type_infer::monomorphize_actor` ~1.93%
- `pcc::type_infer::TypeInferEngine::infer_pipe_expr_with_upstream` ~1.53%
- `pcc::type_infer::TypeInferEngine::get_effective_meta` ~1.27%
- `pcc::resolve::resolve` ~0.56%

## Implication for next optimization work

1. Reduce metadata cloning/allocation churn across compiler phases.
2. Focus algorithm/memory improvements in type-inference path.
3. Continue maintainability/perf cleanup in remaining analyze/codegen hotspots:
   - `check_dim_source_conflicts`
   - `build_schedule_dim_overrides`
   - `format_actor_params`
