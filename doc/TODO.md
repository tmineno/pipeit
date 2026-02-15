# Pipit Development Roadmap

**v0.1.0 Tagged** ✅ (2026-02-15)

- Full compiler pipeline (parse → resolve → graph → analyze → schedule → codegen)
- Runtime library with lock-free ring buffers, timers, statistics
- 265 passing tests, comprehensive documentation
- Basic benchmarks (compiler + runtime primitives + end-to-end PDL)

---

## v0.1.1 - Probe Completion & Hardening (Current)

**Goal**: Complete probe runtime wiring and harden startup/runtime behavior with full end-to-end test coverage.

### Runtime Features

- [x] **Probe runtime wiring and startup validation**:
  - [x] Build runtime map from probe name → generated `_probe_<name>_enabled` flag
  - [x] Wire `--probe <name>` to enable matching probe flags before task launch
  - [x] On unknown `--probe <name>`: print startup error and exit code `2`
  - [x] Support multiple `--probe` flags; duplicate names are idempotent
  - [x] Wire `--probe-output <file>` to `_probe_output` `FILE*` before task launch
  - [x] On `--probe-output` file open failure: print startup error and exit code `2` (hard-fail, no fallback)
  - [x] Keep default probe output on `stderr` when `--probe-output` is not provided
  - [x] Keep probe emission guarded for release builds (`#ifndef NDEBUG`)

- [x] **Probe completion exit criteria**:
  - [x] Probe data is emitted only for explicitly enabled probe names
  - [x] Startup validation failures never launch worker threads
  - [x] Probe startup and runtime behavior documented in `doc/pcc-usage-guide.md`

### Quality & Testing

- [x] **End-to-end tests**:
  - [x] Test receiver.pdl compiles and runs
  - [x] Test --stats output format (task stats + shared buffer stats)
  - [x] Test probe emits data for enabled probe
  - [x] Test probe remains silent when not enabled
  - [x] Test unknown probe name exits with code `2` and startup error message
  - [x] Test `--probe-output` missing path exits with code `2`
  - [x] Test `--probe-output` open failure exits with code `2` and startup error message
  - [x] Test duplicate `--probe <name>` arguments are accepted and do not duplicate control state

---

## v0.1.2 - Standard Actor Library

**Goal**: Provide well-tested, documented actors for common signal processing tasks. Prioritize simple, high-value actors before complex ones.

### Phase 1: Essential I/O & Math (Simple, High Value) ✅ COMPLETE

- [x] **File I/O** (MEDIUM complexity):
  - [x] `binread(path, dtype)` - Binary file reader (int16, int32, float, cfloat)
  - [x] `binwrite(path, dtype)` - Binary file writer
  - [x] Error handling for file operations (ACTOR_ERROR on failure)
  - [x] Unit tests with known input/output files

- [x] **Standard I/O** (LOW complexity):
  - [x] Enhance `stdout()` with format options (hex, scientific notation) → `stdout_fmt(format)`
  - [x] `stderr()` - Write to stderr for error reporting
  - [x] `stdin()` - Read from stdin (interactive pipelines)

- [x] **Basic arithmetic** (LOW complexity):
  - [x] `sub()` - Subtraction
  - [x] `div()` - Division
  - [x] `abs()` - Absolute value
  - [x] `sqrt()` - Square root
  - [x] Unit tests for each (edge cases: zero, negative, inf, NaN)

- [x] **Basic statistics** (LOW-MEDIUM complexity):
  - [x] `mean(N)` - Running mean over N samples
  - [x] `rms(N)` - RMS over N samples
  - [x] `min(N)` - Minimum over window
  - [x] `max(N)` - Maximum over window
  - [x] Unit tests with known sequences

**Phase 1 Summary:**

- 25 standard actors implemented in `runtime/libpipit/include/std_actors.h`
- 85 integration tests + 58 C++ runtime tests (143 total for stdlib)
- All actors have both compilation and runtime test coverage

### Documentation ✅ COMPLETE

- [x] **Documentation generation**:
  - [x] Convert existing comments to Doxygen format (`/// @brief`, `@param`, `@return`, `@code{.pdl}`)
  - [x] Create `scripts/gen-stdlib-doc.py` to parse Doxygen comments and generate flat markdown
  - [x] Generate `doc/spec/standard-library-spec.md` from `std_actors.h`
  - [x] Add `gen-stdlib-doc` pre-commit hook (triggers on `std_actors.h` changes)

---

## v0.2.0 - Frame Dimension Inference & Vectorization Alignment ✅

**Goal**: Align compiler/runtime architecture with `doc/spec/pipit-lang-spec-v0.2.0.md` shape inference plan before adding more medium/high complexity actors.

### Spec & Scope Lock

- [x] **Freeze v0.2.0 shape model** (`doc/spec/pipit-lang-spec-v0.2.0.md`):
  - [x] Confirm shape semantics: `rate = product(shape)` with flat runtime buffers
  - [x] Confirm backward compatibility: `IN(T, N)` / `OUT(T, N)` as rank-1 shorthand
  - [x] Confirm call-site shape constraint syntax: `actor(...)[d0, d1, ...]`
  - [x] Confirm compile-time-only dimension policy (literal/const only; no runtime param)
  - [x] Create ADR for accepted v0.2.0 constraints and non-goals (`doc/adr/007-shape-inference-v020.md`)

### Compiler Alignment (Current Impl Gap Closure)

- [x] **Registry metadata evolution** (`compiler/src/registry.rs`):
  - [x] Introduce shape-aware port metadata (`PortShape` type alongside `TokenCount`)
  - [x] Support `SHAPE(...)` parsing in `IN/OUT` scanner
  - [x] Preserve compatibility with existing actor headers using scalar count form

- [x] **Parser/AST updates for shape constraints**:
  - [x] Extend actor-call grammar to accept optional shape constraints (`[d0, d1, ...]`)
  - [x] Store shape constraint AST on actor call nodes (`ShapeConstraint`, `ShapeDim`)
  - [x] Restrict shape constraint elements to compile-time integers / const refs

- [x] **Analyze/Schedule updates** (`compiler/src/analyze.rs`, `compiler/src/schedule.rs`):
  - [x] Add shape-aware rate resolution (`resolve_port_rate`)
  - [x] Resolve symbolic dimensions from args or shape constraints before SDF balance solving
  - [x] Emit explicit errors for runtime param used as shape dimension
  - [x] Keep existing multi-input per-edge consumption semantics with divisibility checks

- [x] **Codegen updates** (`compiler/src/codegen.rs`):
  - [x] Use resolved shape product for buffer sizes and actor call strides
  - [x] Ensure generated C++ remains flat-buffer ABI compatible
  - [x] Keep old actor declarations compiling without source changes

### Diagnostics & Tests

- [x] **Diagnostics**:
  - [x] Add dedicated error messages for:
    - [x] runtime param used as shape dimension (`"runtime param '$N' cannot be used as frame dimension"`)
    - [x] unknown name in shape constraint (`"unknown name 'X' in shape constraint"`)
    - [x] unresolved frame dimension (§13.6: actor with no args, no shape constraint, no inference)
    - [x] conflicting frame constraint (§13.6: inferred shape vs explicit shape mismatch)
    - [x] cross-clock rate mismatch error for pipeline tasks (§5.7: `Pw × fw ≠ Cr × fr`)

- [x] **Test coverage**:
  - [x] Add parser tests for `actor()[...]` syntax (7 tests)
  - [x] Add registry tests for `SHAPE(...)` port declarations (9 tests)
  - [x] Add resolve tests for shape constraint validation (6 tests)
  - [x] Add analysis tests for dimension inference success/failure cases (3 tests)
  - [x] Add analysis tests for shape constraint validation (3 tests: unresolved dim, conflicting shape, matching ok)
  - [x] Add analysis tests for Phase 4/5/6 (5 tests: cross-clock rate, buffer size, memory pool)
  - [x] Strengthen existing analysis test assertions with concrete rv values (balance, shape inference)
  - [x] Add codegen compile tests covering inferred vs explicit shape constraints (3 tests)
  - [x] Add migration tests proving v0.1-style programs remain unchanged (all 92 codegen tests pass)
  - [x] Fix runtime actor tests for frame-rate variant actors (constant, mul, c2r now require N=1)

- [x] **Done criteria for v0.2.0**:
  - [x] `doc/spec/pipit-lang-spec-v0.2.0.md` and implementation behavior match
  - [x] No regression in existing examples/tests (353 tests total, all passing)
  - [x] Shape constraint diagnostics implemented in resolve and analyze phases

---

## v0.2.1 - Performance Characterization & Spec Sheet

**Goal**: Establish comprehensive performance baselines and identify bottlenecks. Measure what exists before optimizing.

### Extended Benchmarks

- [x] **Compiler benchmarks** (`compiler/benches/compiler_bench.rs`):
  - [x] Large pipeline compilation (100+ actors, 20+ tasks) — `generate_large_pipeline(20, 5)`
  - [x] Deep nesting (define within define, 5+ levels) — `generate_deep_nesting(5)`
  - [x] Wide fan-out (single source → 50 consumers via taps) — `generate_wide_fanout(50)`
  - [x] Modal complexity (10+ modes, 5+ control signals) — `generate_modal_complex(10)`
  - [x] Incremental compilation time (measure per-phase cost) — `bench_per_phase` (parse/resolve/graph/analyze/schedule/codegen)
  - [x] Full pipeline with loaded registry — `bench_full_pipeline_loaded` (parse through codegen)
  - [x] Parse scaling benchmark — `bench_parse_scaling` (1/5/10/20/50 tasks)

- [x] **Ring buffer stress tests** (`benches/ringbuf_bench.cpp`):
  - [x] High throughput: 1M tokens write+read (writer+reader threads)
  - [x] Multi-reader contention: 2, 4, 8, 16 readers (templated)
  - [x] Buffer size scaling: 64, 256, 1K, 4K, 16K, 64K tokens
  - [x] Chunk size scaling: 1, 4, 16, 64, 256, 1024 tokens per transfer
  - [x] Cache effects: Measure L1/L2/L3 hit rates — `perf/perf_ringbuf.sh` (perf stat on SizeScaling benchmarks)
  - [x] NUMA effects: Cross-socket read/write performance — `perf/perf_numa.sh` (CCD-distance via taskset on single-node; numactl on multi-node)

- [x] **Timer precision benchmarks** (`benches/timer_bench.cpp`):
  - [x] Frequency sweep: 1Hz to 1MHz in 10x steps
  - [x] Jitter measurement: Histogram of tick latency (10k ticks, percentile breakdown)
  - [ ] Long-running stability: 24-hour drift test (deferred — too long for CI)
  - [x] Overrun recovery: Measure slip/backlog behavior (force overrun + reset_phase)
  - [x] Thread wake-up latency: Best/worst/median (1k ticks at 1kHz)

- [x] **Task scheduling overhead** (`benches/thread_bench.cpp`):
  - [x] Thread creation/join cost
  - [x] Context switch overhead between tasks (atomic ping-pong)
  - [x] Empty pipeline (minimal actor work, measure framework overhead)
  - [x] Scaling: 1, 2, 4, 8, 16, 32 concurrent tasks
  - [x] Timer overhead (pure timer object cost, no sleep)
  - [x] CPU affinity impact on performance — `affinity_bench.cpp` + `perf/perf_affinity.sh` (topology probed from sysfs)

- [x] **Memory subsystem** (`benches/memory_bench.cpp` + `perf/perf_memory.sh`):
  - [x] Total memory footprint per task — `BM_Memory_Footprint` (sizeof via counters)
  - [x] Cache line utilization in RingBuffer — `BM_Memory_CacheLineUtil` + perf L1-dcache analysis
  - [x] False sharing detection — `BM_Memory_FalseSharing` (1-8 readers) + perf cache-miss scaling
  - [x] Memory bandwidth saturation point — `BM_Memory_Bandwidth` (4KB-16MB memcpy)
  - [x] Page fault impact — `BM_Memory_PageFault_Cold/Warm` + perf page-fault counters

- [x] **Actor performance** (`benches/actor_bench.cpp`):
  - [x] Per-actor microbenchmarks (FFT, FIR, mul, add, sub, div, abs, sqrt, mean, rms, min, max, c2r, mag, decimate)
  - [x] FFT scaling: N=64, 256, 1024, 4096
  - [x] FIR tap scaling: 5-tap, 16-tap, 64-tap
  - [x] Vectorization effectiveness — `perf/perf_actor.sh` (IPC as SIMD proxy across actors/FFT/FIR)
  - [x] Pipeline stalls (data dependencies, cache misses) — `perf/perf_actor.sh` (L1-dcache, stalled-cycles)
  - [ ] Actor fusion potential (requires schedule-fusion implementation)

- [x] **End-to-end workloads** (`benches/pdl/`):
  - [x] SDR receiver chain (1 MHz capture + 100 kHz demod) — `sdr_receiver.pdl`
  - [x] Audio processing (48 kHz effects chain) — `audio_chain.pdl`
  - [x] Sensor fusion (5 sensors @ 1 kHz + aggregator) — `sensor_fusion.pdl`

### Profiling & Analysis

- [x] **Profiling with perf** (`perf/perf_profile.sh` + `perf/perf_flamegraph.sh`):
  - [x] CPU hotspots — perf record + perf report (per benchmark binary)
  - [x] Branch mispredictions — perf stat branch-misses
  - [x] Cache misses (L1/L2/L3) — perf stat L1-dcache-load-misses + cache-misses
  - [x] TLB misses — perf stat dTLB/iTLB-load-misses
  - [x] Flame graphs for representative workloads — SVG via FlameGraph tools

- [x] **Lock contention analysis** (`perf/perf_contention.sh`):
  - [x] Atomic contention in RingBuffer (multi-reader) — cache-miss scaling as reader count grows
  - [x] Memory ordering overhead — 1-reader vs multi-reader IPC comparison
  - [x] Lock-free algorithm inefficiencies — stalled-cycles + cache-miss proxy analysis

- [x] **Latency breakdown** (`benches/latency_bench.cpp`):
  - [x] Time per actor firing (min/avg/max/p99) — percentile tracking for mul, fft, fir, mean, c2r, rms
  - [x] Timer overhead vs actual work — timer.wait() vs actor compute ratio
  - [x] Ring buffer read/write vs compute — component-level budget breakdown
  - [x] Task wake-up to first instruction — thread creation to first timestamp
  - [x] End-to-end latency budget (source → sink) — mul→fir→mean pipeline budget

- [ ] **Comparison with alternatives** (deferred — requires GNU Radio setup):
  - [ ] GNU Radio: Same pipeline, compare throughput
  - [ ] Pure C++ hand-written: Measure framework overhead
  - [ ] Theoretical maximum (FLOPS, memory bandwidth)

### Documentation

- [ ] **Performance spec sheet** (`doc/PERFORMANCE.md`):
  - [ ] Hardware tested (CPU, cache, RAM, OS)
  - [ ] Compiler performance (parse time, memory, codegen size)
  - [ ] Runtime performance (ring buffer, timer, task overhead)
  - [ ] Scaling characteristics (tasks, pipeline depth, buffer sizes)
  - [ ] Real-world workload benchmarks
  - [ ] Comparison table (Pipit vs GNU Radio vs hand-coded)
  - [ ] Known limitations (frequency limits, buffer constraints)

- [ ] **Performance tuning guide** (`doc/tuning.md`):
  - [ ] Buffer sizing guidelines
  - [ ] Thread/task mapping best practices
  - [ ] Overrun policy selection criteria
  - [ ] CPU affinity and NUMA considerations
  - [ ] Compiler optimization flags

- [x] **Benchmark automation**:
  - [x] Add `benches/run_all.sh` wrapper to run all benchmark suites
  - [x] JSON output format for results (Google Benchmark JSON + Criterion HTML)
  - [ ] Regression detection (compare against baseline)
  - [ ] CI integration: Track performance over commits

---

## v0.2.2 - Standard Actor Library (Continuation)

### Phase 2: Signal Processing Basics (Medium Complexity)

- [ ] **Simple filters** (MEDIUM complexity):
  - [ ] `lpf(cutoff, order)` - Low-pass filter (Butterworth)
  - [ ] `hpf(cutoff, order)` - High-pass filter
  - [ ] `notch(freq, q)` - Notch filter
  - [ ] Test with known signal characteristics

- [ ] **Basic transforms** (MEDIUM-HIGH complexity):
  - [ ] `ifft(N)` - Inverse FFT
  - [ ] `rfft(N)` - Real FFT (optimize for real input)
  - [ ] Validate against reference implementations (e.g., FFTW)

- [ ] **Windowing** (LOW-MEDIUM complexity):
  - [ ] `window(N, type)` - Window functions (hann, hamming, blackman)
  - [ ] Test window properties (energy, side lobe levels)

### Phase 3: Advanced Signal Processing (High Complexity)

- [ ] **WAV file I/O** (MEDIUM-HIGH complexity):
  - [ ] `wavread(path)` - WAV file reader (streaming, 16/24/32-bit PCM)
  - [ ] `wavwrite(path)` - WAV file writer (configurable sample rate, channels)
  - [ ] Handle WAV header parsing, endianness

- [ ] **Advanced filters** (HIGH complexity):
  - [ ] `iir(b_coeff, a_coeff)` - IIR filter (biquad sections)
  - [ ] `bpf(low, high, order)` - Band-pass filter
  - [ ] Numerical stability testing

- [ ] **Resampling** (HIGH complexity):
  - [ ] `resample(M, N)` - Rational resampling (upsample M, downsample N)
  - [ ] `interp(N)` - Interpolation (zero-insert + LPF)
  - [ ] `downsample(N)` - Downsampling (LPF + decimate)

- [ ] **Advanced transforms** (HIGH complexity):
  - [ ] `dct(N)` - Discrete Cosine Transform
  - [ ] `hilbert(N)` - Hilbert transform (analytic signal)
  - [ ] `stft(N, hop)` - Short-Time Fourier Transform
  - [ ] `istft(N, hop)` - Inverse STFT

- [ ] **Advanced statistics** (MEDIUM complexity):
  - [ ] `var(N)` - Variance
  - [ ] `std(N)` - Standard deviation
  - [ ] `xcorr(N)` - Cross-correlation
  - [ ] `acorr(N)` - Auto-correlation
  - [ ] `convolve(N, kernel)` - Convolution

- [ ] **Control flow** (MEDIUM complexity):
  - [ ] `gate(threshold)` - Pass/block based on signal level
  - [ ] `clipper(min, max)` - Hard clipping
  - [ ] `limiter(threshold)` - Soft limiting
  - [ ] `agc(target, attack, release)` - Automatic Gain Control

### Infrastructure & Documentation

- [ ] **Testing infrastructure**:
  - [ ] Per-actor unit test framework
  - [ ] Test harness for actor correctness
  - [ ] Edge case testing (zero, infinity, NaN)
  - [ ] Performance tests (measure ns/firing)

- [ ] **Actor documentation**:
  - [ ] API reference template
  - [ ] Usage examples for each actor
  - [ ] Performance characteristics
  - [ ] Known limitations

- [ ] **Example pipelines**:
  - [ ] Audio effects (basic filters, gain)
  - [ ] SDR examples (if filters/transforms complete)
  - [ ] Simple sensor processing

- [ ] **Actor header organization**:
  - [ ] Split `actors.h` into categories: `io.h`, `filters.h`, `math.h`, etc.
  - [ ] Maintain `actors.h` as umbrella include
  - [ ] Consider `--actor-path` for automatic discovery

---

## v0.3.0 - Language Evolution (Type Inference)

**Goal**: Improve PDL ergonomics and type system based on real usage experience. Design-first approach.

### Phase 1: Design & Specification

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

### Phase 2: Implementation (After Spec Approval)

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
  - [ ] Monomorphization during codegen

- [ ] **Better type error messages**:
  - [ ] Show expected vs actual types in context
  - [ ] Suggest conversion actors with exact syntax
  - [ ] Trace type through pipeline
  - [ ] Highlight problematic pipe operator in source

---

## v0.3.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

### Runtime Improvements

- [ ] Round-robin scheduler with thread pools
- [ ] Platform support (macOS, Windows native)
- [ ] LSP server for IDE integration
- [ ] Improved documentation and tutorials

### Build & Distribution

- [ ] CMake integration for actor library
- [ ] Install target (copy headers to system include path)
- [ ] pkg-config support
- [ ] Package manager for actor distribution

---

## v0.4.0 - Advanced Features (Future)

**Goal**: Compiler optimizations, real-time scheduling, heterogeneous execution.

### Compiler Optimizations

- [ ] Fusion (merge adjacent actors)
- [ ] Constant propagation
- [ ] Dead code elimination
- [ ] Actor inlining

### Real-time Scheduling

- [ ] Priority-based scheduling
- [ ] Deadline guarantees
- [ ] CPU affinity control
- [ ] NUMA-aware placement

### Heterogeneous Execution

- [ ] GPU support (CUDA, OpenCL)
- [ ] FPGA code generation
- [ ] Accelerator offload

### Distributed Computing

- [ ] Distributed pipelines across nodes
- [ ] Network-transparent buffers
- [ ] Fault tolerance and checkpointing

---

## v0.5.0 - Production Hardening (Future)

**Goal**: Observability, reliability, security, verification for production deployments.

### Observability

- [ ] Metrics and monitoring (Prometheus, Grafana, OpenTelemetry)
- [ ] Built-in profiler and debugger
- [ ] Distributed tracing

### Reliability

- [ ] Fault tolerance
- [ ] State checkpointing and recovery
- [ ] Graceful degradation

### Security

- [ ] Sandboxing
- [ ] Input validation
- [ ] Resource limits

### Verification

- [ ] Property-based testing
- [ ] Formal verification of scheduling
- [ ] Model checking for deadlock freedom

---

## Notes

- **v0.1.1** completes runtime features designed for v0.1.0 - essential foundation
- **v0.1.2** closes the first standard-library milestone (Phase 1 + docs)
- **v0.2.0** ✅ aligned implementation with frame-dimension/vectorization plan (ADR-007, PortShape, SHAPE parsing, shape constraints, dimension inference, §13.6 shape validation, §5.7 cross-clock rate enforcement)
- **v0.2.1-v0.2.2** continue stdlib expansion and performance baselining
- **v0.3.0** keeps language evolution as design-first (spec/ADR before implementation)
- **v0.4.0+** deferred until core is stable and well-characterized
- Performance characterization should inform optimization priorities (measure before optimizing)
