# ADR-014: Adaptive Spin-Wait Defaults & EWMA Jitter Calibration

## Context

v0.2.1 benchmark runs revealed that the current defaults (`tick_rate = 1MHz`, `timer_spin = 0`) cause unnecessary OS wake-ups and leave wake-up jitter unmitigated:

- At 100kHz with K=1, overrun rate is 87.65% and CPU time is 105ms.
- At 10kHz with K=1, overrun rate is 2.85% with p99 latency ~111us.
- With K=10 (via `set tick_rate = 1kHz`), overruns drop to 0 and CPU time drops 8.5x.
- With `timer_spin = 10us`, overruns at 10kHz drop from 52 to 18.

Users must currently know their OS timer granularity and manually tune `tick_rate` and `timer_spin` to achieve good results. The safe-looking defaults (1MHz tick, no spin) produce poor behavior at common DSP rates.

## Decision

### 1. Change `tick_rate` default: 1MHz → 10kHz

For tasks at or below 10kHz (the practical OS timer limit), K remains 1. For higher-frequency tasks, the compiler automatically computes K > 1, amortizing wake-up overhead. This matches the measured sweet spot where overrun rate stays below 3%.

### 2. Change `timer_spin` default: 0 → 10000 (10us)

A 10us spin window reduces overruns with minimal CPU impact. Benchmark data shows this cuts overruns from 52 to 18 at 10kHz while reducing p99 latency from 113.9us to 99.1us.

### 3. Add `set timer_spin = auto` for EWMA-based runtime calibration

When `timer_spin = auto` is specified in PDL, the compiler generates `spin_ns = -1` as a sentinel. The C++ Timer then:

- Bootstraps with `spin_threshold = 10us` (same as fixed default).
- After each `sleep_until()`, measures wake-up jitter.
- Updates an EWMA: `ewma += (sample - ewma) / 8` (alpha = 1/8, integer arithmetic).
- Sets `spin_threshold = clamp(2 * ewma, 500ns, 100us)`.

This adapts to the platform's sleep granularity without manual tuning.

### 4. Add compile-time rate guardrails

The scheduler emits warnings when:

- Effective tick period < 10us (>100kHz timer wake): most OS schedulers cannot sustain this.
- Fixed `timer_spin` exceeds 50% of tick period: chronic overruns likely.

## Consequences

- **Breaking default change**: Existing PDL without explicit `set tick_rate` will get `K = ceil(task_freq / 10kHz)` instead of `K = ceil(task_freq / 1MHz)`. Users with explicit `set tick_rate` are unaffected.
- **Behavioral default change**: Programs without `set timer_spin` will now spin-wait 10us before each deadline instead of using pure `sleep_until()`. CPU usage increases slightly; jitter decreases.
- **Adaptive mode overhead**: ~2ns per-tick (one integer subtraction + divide-by-8). Negligible relative to microsecond-scale spin windows.
- **No ABI break**: Timer constructor signature is unchanged (`int64_t spin_ns`). Negative values activate adaptive mode.

## Alternatives

- **Startup-only calibration**: Rejected — jitter varies with system load over time; EWMA adapts continuously.
- **Compile-time platform heuristic**: Rejected — cross-compilation makes this unreliable; WSL2/native Linux/macOS have very different jitter profiles.
- **Keep 1MHz default**: Rejected — causes chronic overruns at common audio/DSP rates (48kHz, 100kHz).
- **PID controller for spin adaptation**: Rejected — overkill for a monotonic signal; EWMA is simpler and sufficient.

## Exit criteria

- [ ] Adaptive p99 jitter within 2x of platform-optimal fixed spin on benchmark.
- [ ] Adaptive CPU overhead < 5% higher than optimal fixed spin.
- [ ] All existing tests pass with new defaults (golden file updates accepted).
- [ ] Rate guardrail warns for effective period < 10us.
- [ ] `set timer_spin = auto` documented in spec §5.1.
