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

- [ ] **Probe runtime wiring and startup validation**:
  - [ ] Build runtime map from probe name → generated `_probe_<name>_enabled` flag
  - [ ] Wire `--probe <name>` to enable matching probe flags before task launch
  - [ ] On unknown `--probe <name>`: print startup error and exit code `2`
  - [ ] Support multiple `--probe` flags; duplicate names are idempotent
  - [ ] Wire `--probe-output <file>` to `_probe_output` `FILE*` before task launch
  - [ ] On `--probe-output` file open failure: print startup error and exit code `2` (hard-fail, no fallback)
  - [ ] Keep default probe output on `stderr` when `--probe-output` is not provided
  - [ ] Keep probe emission guarded for release builds (`#ifndef NDEBUG`)

- [ ] **Probe completion exit criteria**:
  - [ ] Probe data is emitted only for explicitly enabled probe names
  - [ ] Startup validation failures never launch worker threads
  - [ ] Probe startup and runtime behavior documented in `doc/pcc-usage-guide.md`

### Quality & Testing

- [ ] **End-to-end tests**:
  - [ ] Test receiver.pdl compiles and runs
  - [ ] Test --stats output format (task stats + shared buffer stats)
  - [ ] Test probe emits data for enabled probe
  - [ ] Test probe remains silent when not enabled
  - [ ] Test unknown probe name exits with code `2` and startup error message
  - [ ] Test `--probe-output` missing path exits with code `2`
  - [ ] Test `--probe-output` open failure exits with code `2` and startup error message
  - [ ] Test duplicate `--probe <name>` arguments are accepted and do not duplicate control state

---

## v0.1.2 - Standard Actor Library

**Goal**: Provide well-tested, documented actors for common signal processing tasks. Prioritize simple, high-value actors before complex ones.

### Phase 1: Essential I/O & Math (Simple, High Value)

- [ ] **File I/O** (MEDIUM complexity):
  - [ ] `binread(path, dtype)` - Binary file reader (int16, int32, float, cfloat)
  - [ ] `binwrite(path, dtype)` - Binary file writer
  - [ ] Error handling for file operations (ACTOR_ERROR on failure)
  - [ ] Unit tests with known input/output files

- [ ] **Standard I/O** (LOW complexity):
  - [ ] Enhance `stdout()` with format options (hex, scientific notation)
  - [ ] `stderr()` - Write to stderr for error reporting
  - [ ] `stdin()` - Read from stdin (interactive pipelines)

- [ ] **Basic arithmetic** (LOW complexity):
  - [ ] `sub()` - Subtraction
  - [ ] `div()` - Division
  - [ ] `abs()` - Absolute value
  - [ ] `sqrt()` - Square root
  - [ ] Unit tests for each (edge cases: zero, negative, inf, NaN)

- [ ] **Basic statistics** (LOW-MEDIUM complexity):
  - [ ] `mean(N)` - Running mean over N samples
  - [ ] `rms()` - RMS (verify existing implementation)
  - [ ] `min(N)` - Minimum over window
  - [ ] `max(N)` - Maximum over window
  - [ ] Unit tests with known sequences

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

## v0.1.3 - Performance Characterization & Spec Sheet

**Goal**: Establish comprehensive performance baselines and identify bottlenecks. Measure what exists before optimizing.

### Extended Benchmarks

- [ ] **Compiler benchmarks**:
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
  - [ ] Total memory footprint per task
  - [ ] Cache line utilization in RingBuffer
  - [ ] False sharing detection
  - [ ] Memory bandwidth saturation point
  - [ ] Page fault impact

- [ ] **Actor performance**:
  - [ ] Per-actor microbenchmarks (FFT, FIR, mul, add, etc.)
  - [ ] Vectorization effectiveness
  - [ ] Pipeline stalls (data dependencies, cache misses)
  - [ ] Actor fusion potential

- [ ] **End-to-end workloads**:
  - [ ] SDR receiver chain (1 MSPS, 10 MSPS, 100 MSPS)
  - [ ] Audio processing (48 kHz, 16-bit stereo, 10 effects)
  - [ ] Sensor fusion (10 sensors @ 1 kHz)

### Profiling & Analysis

- [ ] **Profiling with perf**:
  - [ ] CPU hotspots
  - [ ] Branch mispredictions
  - [ ] Cache misses (L1/L2/L3)
  - [ ] TLB misses
  - [ ] Flame graphs for representative workloads

- [ ] **Lock contention analysis**:
  - [ ] Atomic contention in RingBuffer (multi-reader)
  - [ ] Memory ordering overhead
  - [ ] Lock-free algorithm inefficiencies

- [ ] **Latency breakdown**:
  - [ ] Time per actor firing (min/avg/max/p99)
  - [ ] Timer overhead vs actual work
  - [ ] Ring buffer read/write vs compute
  - [ ] Task wake-up to first instruction
  - [ ] End-to-end latency budget (source → sink)

- [ ] **Comparison with alternatives**:
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

- [ ] **Benchmark automation**:
  - [ ] Add `benches/run_all.sh` wrapper to run `benches/pdl_bench.sh` plus runtime benchmark build/run
  - [ ] JSON output format for results
  - [ ] Regression detection (compare against baseline)
  - [ ] CI integration: Track performance over commits

---

## v0.2.0 - Language Evolution (Type Inference)

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

## v0.2.x - Ecosystem & Quality of Life

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

## v0.3.0 - Advanced Features (Future)

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

## v0.4.0 - Production Hardening (Future)

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
- **v0.1.2-v0.1.3** build standard library and performance baselines before language changes
- **v0.2.0** uses design-first approach (spec/ADR before implementation)
- **v0.3.0+** deferred until core is stable and well-characterized
- Performance characterization should inform optimization priorities (measure before optimizing)
