# Pipit Development Roadmap

**v0.1.0 Complete** ✅ (2026-02-15)

- Full compiler pipeline (parse → resolve → graph → analyze → schedule → codegen)
- Runtime library with lock-free ring buffers, timers, statistics
- 265 passing tests, comprehensive documentation
- Basic benchmarks (compiler + runtime primitives + end-to-end PDL)

---

## v0.1.1 - Performance Characterization & Spec Sheet (Current)

**Goal**: Establish comprehensive performance baselines and identify bottlenecks before major feature additions.

### Wide Coverage Runtime Benchmarks

- [ ] **Extended compiler benchmarks**:
  - [ ] Large pipeline compilation (100+ actors, 20+ tasks)
  - [ ] Deep nesting (define within define, 5+ levels)
  - [ ] Wide fan-out (single source → 50 consumers via taps)
  - [ ] Modal complexity (10+ modes, 5+ control signals)
  - [ ] Incremental compilation time (measure per-phase cost)

- [ ] **Ring buffer stress tests**:
  - [ ] High throughput: 1M tokens/sec write+read
  - [ ] Multi-reader contention: 2, 4, 8, 16 readers
  - [ ] Buffer size scaling: 64, 256, 1K, 4K, 16K, 64K tokens
  - [ ] Cache effects: Measure L1/L2/L3 hit rates
  - [ ] NUMA effects: Cross-socket read/write performance

- [ ] **Timer precision benchmarks**:
  - [ ] Frequency sweep: 1Hz to 1MHz in 10x steps
  - [ ] Jitter measurement: Histogram of tick latency
  - [ ] Long-running stability: 24-hour drift test
  - [ ] Overrun recovery: Measure slip/backlog behavior
  - [ ] Thread wake-up latency: Best/worst/median

- [ ] **Task scheduling overhead**:
  - [ ] Thread creation/join cost
  - [ ] Context switch overhead between tasks
  - [ ] Empty pipeline (minimal actor work, measure framework overhead)
  - [ ] Scaling: 1, 2, 4, 8, 16, 32 concurrent tasks
  - [ ] CPU affinity impact on performance

- [ ] **Memory subsystem**:
  - [ ] Total memory footprint per task (measure with /proc/self/status)
  - [ ] Cache line utilization in RingBuffer
  - [ ] False sharing detection (shared buffers on same cache line)
  - [ ] Memory bandwidth saturation point
  - [ ] Page fault impact (large buffers, first access)

- [ ] **Actor performance**:
  - [ ] Per-actor microbenchmarks (FFT, FIR, mul, add, etc.)
  - [ ] Vectorization effectiveness (check assembly, measure speedup)
  - [ ] Pipeline stalls (data dependencies, cache misses)
  - [ ] Actor fusion potential (measure overhead of actor boundaries)

- [ ] **End-to-end workloads**:
  - [ ] SDR receiver chain (1 MSPS, 10 MSPS, 100 MSPS)
  - [ ] Audio processing (48 kHz, 16-bit stereo, 10 effects)
  - [ ] Video frame processing (1080p@30fps, simple filters)
  - [ ] Sensor fusion (10 sensors @ 1 kHz, Kalman filter)
  - [ ] Network packet processing (1 Gbps, 10 Gbps theoretical)

### Bottleneck Analysis

- [ ] **Profiling with perf**:
  - [ ] CPU hotspots: Which functions consume most time?
  - [ ] Branch mispredictions: Control flow efficiency
  - [ ] Cache misses: L1/L2/L3 miss rates per component
  - [ ] TLB misses: Page table efficiency
  - [ ] Generate flame graphs for representative workloads

- [ ] **Lock contention analysis**:
  - [ ] Measure atomic contention in RingBuffer (multi-reader)
  - [ ] Memory ordering overhead (relaxed vs acquire/release)
  - [ ] Identify lock-free algorithm inefficiencies

- [ ] **Latency breakdown**:
  - [ ] Time per actor firing (min/avg/max/p99)
  - [ ] Timer overhead vs actual work
  - [ ] Ring buffer read/write vs compute
  - [ ] Task wake-up to first instruction executed
  - [ ] End-to-end latency budget (source → sink)

- [ ] **Comparison with alternatives**:
  - [ ] GNU Radio: Same pipeline, compare throughput
  - [ ] Pure C++ hand-written: Measure framework overhead
  - [ ] Theoretical maximum (FLOPS, memory bandwidth)

### Performance Spec Sheet

Create `doc/PERFORMANCE.md` with:

- [ ] **Hardware tested**:
  - [ ] CPU model, core count, frequency
  - [ ] Cache sizes (L1/L2/L3)
  - [ ] RAM size, speed, channels
  - [ ] OS, kernel version, governor settings

- [ ] **Compiler performance**:
  - [ ] Parse time vs program size (tokens, lines, actors)
  - [ ] Memory usage during compilation
  - [ ] Codegen size vs program complexity
  - [ ] Time breakdown per compiler phase (table)

- [ ] **Runtime performance**:
  - [ ] Ring buffer throughput (tokens/sec, MB/s)
    - Single reader, multi-reader (2/4/8)
    - Different buffer sizes
  - [ ] Timer precision (jitter distribution, histogram)
  - [ ] Task startup overhead (time to first tick)
  - [ ] Empty pipeline overhead (baseline cost)
  - [ ] Per-actor performance (ns/firing for each actor)

- [ ] **Scaling characteristics**:
  - [ ] Throughput vs number of tasks (1-32)
  - [ ] Latency vs pipeline depth (1-100 actors)
  - [ ] Memory vs buffer sizes and task count
  - [ ] Efficiency vs CPU utilization (ideal = 100%)

- [ ] **Real-world workload benchmarks**:
  - [ ] SDR receiver: Max sample rate sustained
  - [ ] Audio: Max concurrent tracks/effects
  - [ ] Video: Max resolution/framerate
  - [ ] Identify bottleneck in each case

- [ ] **Comparison table**:
  - [ ] Pipit vs GNU Radio vs hand-coded C++
  - [ ] For 3-5 representative pipelines
  - [ ] Metrics: throughput, latency, CPU%, memory

- [ ] **Known limitations**:
  - [ ] Frequency limits (min/max sustainable)
  - [ ] Buffer size constraints
  - [ ] Platform-specific issues (Linux vs macOS vs Windows)

### Documentation Updates

- [ ] **Benchmark README** (`benches/README.md`):
  - [ ] Add all new benchmarks to documentation
  - [ ] Explain what each benchmark measures
  - [ ] How to run comprehensive benchmark suite
  - [ ] How to reproduce spec sheet results

- [ ] **Performance tuning guide** (`doc/tuning.md`):
  - [ ] Buffer sizing guidelines
  - [ ] Thread/task mapping best practices
  - [ ] Overrun policy selection criteria
  - [ ] CPU affinity and NUMA considerations
  - [ ] Compiler optimization flags (--march, -flto, PGO)

- [ ] **Known issues** (`doc/known-issues.md`):
  - [ ] Document any performance cliffs discovered
  - [ ] Workarounds for common bottlenecks
  - [ ] Platform-specific quirks

### Tooling

- [ ] **Benchmark automation**:
  - [ ] `benches/run_all.sh` - Run full suite, generate report
  - [ ] JSON output format for results
  - [ ] Regression detection (compare against baseline)
  - [ ] CI integration: Track performance over commits

- [ ] **Profiling helpers**:
  - [ ] `scripts/profile.sh <pdl_file>` - Auto-profile with perf
  - [ ] Flame graph generation
  - [ ] Cache miss visualization

---

## Future Releases (Deferred)

### v0.2.0 - Quality of Life & Language Improvements

#### Type Inference Improvements

##### Phase 1: Design & Specification

- [ ] **Review current type system**:
  - [ ] Document how types currently work (explicit only, no inference)
  - [ ] Identify pain points from user experience (manual c2r() insertion)
  - [ ] Survey type systems in similar DSLs (GNU Radio, Faust, StreamIt)
  - [ ] Collect real-world examples where inference would help

- [ ] **Type inference design discussion**:
  - [ ] Explicit vs implicit conversions trade-offs
  - [ ] Safety: When should implicit conversion be allowed?
  - [ ] Clarity: Does auto-conversion make pipelines harder to understand?
  - [ ] Performance: Cost of implicit conversions vs manual specification
  - [ ] **Create ADR**: Document design decisions and rationale

- [ ] **Spec update proposal** (`doc/spec/pipit-lang-spec-v0.2.0.md`):
  - [ ] Formal type inference rules
  - [ ] Conversion hierarchy (int32 → float → cfloat)
  - [ ] Where inference applies (const/param vs pipelines)
  - [ ] Error message improvements
  - [ ] Generic actor syntax (if supported)
  - [ ] Backwards compatibility considerations

##### Phase 2: Implementation (after spec approval)

- [ ] **Implicit type conversions in pipelines**:
  - [ ] Auto-insert `c2r()` when connecting `cfloat → float` pipeline
  - [ ] Type promotion: `int → float` where compatible
  - [ ] Detect and suggest conversion actors for type mismatches
  - [ ] Warning for lossy conversions (float → int)

- [ ] **Type inference for constants and parameters**:
  - [ ] Infer `const` type from initializer: `const x = 1.0` → float
  - [ ] Infer `param` type from default value: `param gain = 2.5` → float
  - [ ] Support explicit type annotations: `const x: int32 = 42`
  - [ ] Type checking for array elements: `[1, 2.0]` → error (mixed types)

- [ ] **Generic actor support** (if spec approved):
  - [ ] Template-like syntax: `fir<float>(N, coeff)` vs `fir<int32>(N, coeff)`
  - [ ] Type parameter inference from arguments
  - [ ] Monomorphization during codegen (generate specialized versions)

- [ ] **Better type error messages**:
  - [ ] Show expected vs actual types in context
  - [ ] Suggest conversion actors with exact syntax
  - [ ] Trace type through pipeline: `adc(0): float → mul(2.0): float → c2r(): ERROR`
  - [ ] Highlight the problematic pipe operator in source

#### Runtime & Ecosystem

- [ ] Round-robin scheduler with thread pools
- [ ] Platform support (macOS, Windows native)
- [ ] Actor library expansion (file I/O, network, signal processing)
- [ ] LSP server for IDE integration
- [ ] Package manager for actor distribution
- [ ] Improved documentation and tooling

### v0.3 - Advanced Features

- Compiler optimizations (fusion, constant propagation, dead code elimination)
- Real-time scheduling (priority, deadlines, CPU affinity)
- Heterogeneous execution (GPU, FPGA support)
- Distributed pipelines across nodes
- Auto-tuning based on profiling data

### v0.4 - Production Ready

- Metrics and monitoring (Prometheus, Grafana, OpenTelemetry)
- Built-in profiler and debugger
- Fault tolerance and checkpointing
- Security (sandboxing, input validation)
- Property-based testing and formal verification

---

## Notes

- v0.1.1 is about understanding current performance limits, not adding features
- Spec sheet should be reproducible on reference hardware
- Identify bottlenecks before optimization to avoid premature optimization
- Results will guide v0.2 priorities (fix bottlenecks vs add features)
