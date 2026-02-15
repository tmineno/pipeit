# Pipit Standard Library Specification v0.1.2

**Status:** Released
**Date:** 2026-02-15
**Location:** `runtime/libpipit/include/std_actors.h`

---

## 1. Introduction

### 1.1 Purpose

This document specifies the behavior, interface, and semantics of the Pipit Standard Actor Library v0.1.2. The standard library provides well-tested, documented actors for common signal processing tasks.

### 1.2 Scope

This specification covers 25 standard actors across 8 categories:

- Source actors (2)
- Transform actors (4)
- Arithmetic actors (7)
- Statistics actors (4)
- Feedback actors (1)
- File I/O actors (2)
- Rate conversion actors (1)
- Sink actors (4)

### 1.3 Design Principles

1. **Safety**: Actors handle edge cases gracefully (NaN, infinity, division by zero)
2. **Simplicity**: Minimal parameters, clear behavior
3. **Compatibility**: All actors follow ACTOR macro conventions
4. **Testability**: Each actor has compilation and runtime tests
5. **Documentation**: Inline comments and usage examples

### 1.4 Conventions

**Type Notation:**

- `float` - 32-bit IEEE 754 floating point
- `cfloat` - Complex float (two 32-bit floats: real, imaginary)
- `int32` - 32-bit signed integer
- `int16` - 16-bit signed integer
- `void` - No data

**Parameter Types:**

- `PARAM(type, name)` - Compile-time constant parameter
- `RUNTIME_PARAM(type, name)` - Runtime-adjustable parameter
- `IN(type, count)` - Input token specification
- `OUT(type, count)` - Output token specification

**Return Values:**

- `ACTOR_OK` (0) - Successful execution
- `ACTOR_ERROR` (1) - Error condition (halts pipeline)

---

## 2. Source Actors

Source actors generate signals with no input.

### 2.1 constant(value)

**Signature:**

```cpp
ACTOR(constant, IN(void, 0), OUT(float, 1), RUNTIME_PARAM(float, value))
```

**Parameters:**

- `value` (float, runtime) - Constant output value

**Behavior:**

- Outputs the constant `value` on every firing
- Ignores input (void source)

**Error Handling:**

- Always returns `ACTOR_OK`
- Accepts any IEEE 754 float (NaN, inf, -inf, ±0)

**Example:**

```pdl
clock 1kHz t {
    constant(42.0) | stdout()
}
```

**Use Cases:**

- DC signal generation
- Testing and validation
- Gain/offset reference values

---

### 2.2 stdin()

**Signature:**

```cpp
ACTOR(stdin, IN(void, 0), OUT(float, 1))
```

**Behavior:**

- Reads one float value per line from standard input
- Parses input using `scanf("%f", &value)`

**Error Handling:**

- Returns `ACTOR_ERROR` on parse failure (invalid input)
- Returns `ACTOR_ERROR` on EOF (end of input)
- Pipeline halts on error

**Example:**

```pdl
clock 1kHz t {
    stdin() | mul(2.0) | stdout()
}
```

**Use Cases:**

- Interactive pipelines
- Testing with piped input
- Live parameter adjustment

---

## 3. Transform Actors

Transform actors convert signals between representations.

### 3.1 fft(N)

**Signature:**

```cpp
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - FFT size (must be power of 2)

**Behavior:**

- Computes N-point Fast Fourier Transform
- Algorithm: Cooley-Tukey radix-2 decimation-in-time
- Converts real input to complex output

**Error Handling:**

- Returns `ACTOR_ERROR` if N is not a power of 2
- Returns `ACTOR_ERROR` if N ≤ 0

**Numerical Properties:**

- Parseval's theorem: Energy preserved (within floating-point precision)
- Linearity: FFT(a·x + b·y) = a·FFT(x) + b·FFT(y)
- DC bin: out[0] = sum of input samples

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | delay(256, 0.0) | fft(256) | mag() | stdout()
}
```

**Use Cases:**

- Frequency domain analysis
- Spectral processing
- Filter design

---

### 3.2 c2r()

**Signature:**

```cpp
ACTOR(c2r, IN(cfloat, 1), OUT(float, 1))
```

**Behavior:**

- Converts complex to real by computing magnitude
- Equivalent to: `out[0] = std::abs(in[0])`
- Same as `mag()` actor

**Error Handling:**

- Always returns `ACTOR_OK`
- Handles NaN and inf in complex components

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | delay(128, 0.0) | fft(128) | c2r() | stdout()
}
```

**Use Cases:**

- Post-FFT magnitude extraction
- Complex to real conversion
- Type adaptation

---

### 3.3 mag()

**Signature:**

```cpp
ACTOR(mag, IN(cfloat, 1), OUT(float, 1))
```

**Behavior:**

- Computes magnitude of complex signal
- Identical to `c2r()`
- Equivalent to: `out[0] = sqrt(real² + imag²)`

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | delay(64, 0.0) | fft(64) | mag() | stdout()
}
```

---

### 3.4 fir(N, coeff)

**Signature:**

```cpp
ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N) PARAM(std::span<const float>, coeff))
```

**Parameters:**

- `N` (int, compile-time) - Filter length
- `coeff` (float array, compile-time) - Filter coefficients (length N)

**Behavior:**

- Finite Impulse Response filter
- Computes: `out[0] = Σ(coeff[i] * in[i])` for i=0 to N-1
- Consumes N tokens, produces 1 token

**Error Handling:**

- Always returns `ACTOR_OK`
- No bounds checking (N must match coeff length)

**Example:**

```pdl
const mavg = [0.2, 0.2, 0.2, 0.2, 0.2]
clock 1kHz t {
    constant(1.0) | delay(5, 0.0) | fir(5, mavg) | stdout()
}
```

**Use Cases:**

- Moving average filters
- Low-pass filtering
- Signal smoothing

---

## 4. Arithmetic Actors

Basic mathematical operations on float signals.

### 4.1 mul(gain)

**Signature:**

```cpp
ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain))
```

**Parameters:**

- `gain` (float, runtime) - Multiplication factor

**Behavior:**

- Multiplies input by gain: `out[0] = in[0] * gain`

**Error Handling:**

- Always returns `ACTOR_OK`
- Follows IEEE 754 rules (inf * 0 = NaN, etc.)

**Example:**

```pdl
clock 1kHz t {
    constant(10.0) | mul(2.5) | stdout()  # Output: 25.0
}
```

---

### 4.2 add()

**Signature:**

```cpp
ACTOR(add, IN(float, 2), OUT(float, 1))
```

**Behavior:**

- Adds two inputs: `out[0] = in[0] + in[1]`

**Error Handling:**

- Always returns `ACTOR_OK`
- Follows IEEE 754 rules (inf + (-inf) = NaN, etc.)

**Example:**

```pdl
clock 1kHz t {
    constant(3.0) | :a | add(:a)  # Output: 6.0
}
```

---

### 4.3 sub()

**Signature:**

```cpp
ACTOR(sub, IN(float, 2), OUT(float, 1))
```

**Behavior:**

- Subtracts second input from first: `out[0] = in[0] - in[1]`

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(10.0) | :a
    constant(3.0) | :b
    :a | sub(:b)  # Output: 7.0
}
```

---

### 4.4 div()

**Signature:**

```cpp
ACTOR(div, IN(float, 2), OUT(float, 1))
```

**Behavior:**

- Divides first input by second: `out[0] = in[0] / in[1]`
- Division by zero returns `NaN` (quiet failure)

**Error Handling:**

- Always returns `ACTOR_OK`
- Returns `NaN` when `in[1] == 0.0`
- Follows IEEE 754 division rules

**Example:**

```pdl
clock 1kHz t {
    constant(10.0) | :a
    constant(2.0) | :b
    :a | div(:b)  # Output: 5.0
}
```

**Rationale for NaN:**

- Allows pipeline to continue (no hard failure)
- Downstream actors can detect NaN and handle appropriately
- Consistent with IEEE 754 behavior

---

### 4.5 abs()

**Signature:**

```cpp
ACTOR(abs, IN(float, 1), OUT(float, 1))
```

**Behavior:**

- Computes absolute value: `out[0] = |in[0]|`

**Error Handling:**

- Always returns `ACTOR_OK`
- abs(NaN) = NaN
- abs(±inf) = +inf
- abs(±0) = +0

**Example:**

```pdl
clock 1kHz t {
    constant(-42.0) | abs() | stdout()  # Output: 42.0
}
```

---

### 4.6 sqrt()

**Signature:**

```cpp
ACTOR(sqrt, IN(float, 1), OUT(float, 1))
```

**Behavior:**

- Computes square root: `out[0] = √in[0]`
- Negative inputs return `NaN`

**Error Handling:**

- Always returns `ACTOR_OK`
- Returns `NaN` for negative inputs (IEEE 754 behavior)
- sqrt(0) = 0
- sqrt(inf) = inf

**Example:**

```pdl
clock 1kHz t {
    constant(25.0) | sqrt() | stdout()  # Output: 5.0
}
```

---

### 4.7 threshold(value)

**Signature:**

```cpp
ACTOR(threshold, IN(float, 1), OUT(int32, 1), RUNTIME_PARAM(float, value))
```

**Parameters:**

- `value` (float, runtime) - Threshold comparison value

**Behavior:**

- Outputs `1` if `in[0] > value`, otherwise `0`
- Type conversion: float → int32

**Error Handling:**

- Always returns `ACTOR_OK`
- NaN comparisons always return `0` (false)

**Example:**

```pdl
clock 1kHz t {
    constant(1.5) | :signal
    :signal | threshold(1.0) | stdout()  # Output: 1
}
```

**Use Cases:**

- Control signal generation for modal tasks
- Trigger detection
- Binary decisions

---

## 5. Statistics Actors

Window-based statistical operations. All consume N tokens, produce 1 token.

### 5.1 mean(N)

**Signature:**

```cpp
ACTOR(mean, IN(float, N), OUT(float, 1), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - Window size

**Behavior:**

- Computes arithmetic mean: `out[0] = (Σ in[i]) / N`

**Error Handling:**

- Always returns `ACTOR_OK`
- Propagates NaN if any input is NaN

**Example:**

```pdl
clock 1kHz t {
    constant(5.0) | delay(10, 0.0) | mean(10) | stdout()
}
```

---

### 5.2 rms(N)

**Signature:**

```cpp
ACTOR(rms, IN(float, N), OUT(float, 1), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - Window size

**Behavior:**

- Computes root mean square: `out[0] = √((Σ in[i]²) / N)`

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(3.0) | delay(5, 0.0) | rms(5) | stdout()  # Output: 3.0
}
```

**Use Cases:**

- AC signal power measurement
- Signal energy estimation

---

### 5.3 min(N)

**Signature:**

```cpp
ACTOR(min, IN(float, N), OUT(float, 1), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - Window size

**Behavior:**

- Finds minimum value: `out[0] = min(in[0], ..., in[N-1])`

**Error Handling:**

- Always returns `ACTOR_OK`
- Propagates NaN if any input is NaN

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | delay(10, 0.0) | min(10) | stdout()
}
```

---

### 5.4 max(N)

**Signature:**

```cpp
ACTOR(max, IN(float, N), OUT(float, 1), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - Window size

**Behavior:**

- Finds maximum value: `out[0] = max(in[0], ..., in[N-1])`

**Error Handling:**

- Always returns `ACTOR_OK`
- Propagates NaN if any input is NaN

**Example:**

```pdl
clock 1kHz t {
    constant(10.0) | delay(5, 0.0) | max(5) | stdout()
}
```

---

## 6. Feedback Actors

Actors that enable feedback loops in pipelines.

### 6.1 delay(N, init)

**Signature:**

```cpp
ACTOR(delay, IN(float, 1), OUT(float, 1), PARAM(int, N) PARAM(float, init))
```

**Parameters:**

- `N` (int, compile-time) - Number of initial tokens
- `init` (float, compile-time) - Initial token value

**Behavior:**

- Provides N initial tokens with value `init`
- After initial tokens consumed, passes input to output
- Enables feedback loops (breaks circular dependency)

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | :input | add(:feedback)
        | :output | delay(1, 0.0) | :feedback
}
```

**Use Cases:**

- IIR filters
- Recursive structures
- State initialization

---

## 7. File I/O Actors

Binary file reading and writing. Stateful actors (use static FILE*).

### 7.1 binread(path, dtype)

**Signature:**

```cpp
ACTOR(binread, IN(void, 0), OUT(float, 1),
      RUNTIME_PARAM(std::span<const char>, path)
      RUNTIME_PARAM(std::span<const char>, dtype))
```

**Parameters:**

- `path` (string, runtime) - File path
- `dtype` (string, runtime) - Data type ("int16" | "int32" | "float" | "cfloat")

**Behavior:**

- Opens file on first firing (binary read mode)
- Reads one value per firing
- Converts to float output:
  - `int16`, `int32`: Cast to float
  - `float`: Direct copy
  - `cfloat`: Magnitude (|z|)

**Error Handling:**

- Returns `ACTOR_ERROR` on file open failure
- Returns `ACTOR_ERROR` on read error
- Returns `ACTOR_ERROR` on EOF
- Returns `ACTOR_ERROR` on unknown dtype

**Limitations:**

- Stateful (one file per pipeline run)
- File not explicitly closed (relies on OS cleanup)

**Example:**

```pdl
clock 1kHz t {
    binread("data.bin", "int16") | mul(0.001) | stdout()
}
```

**Use Cases:**

- Reading sensor data files
- Batch processing
- Test data playback

---

### 7.2 binwrite(path, dtype)

**Signature:**

```cpp
ACTOR(binwrite, IN(float, 1), OUT(void, 0),
      RUNTIME_PARAM(std::span<const char>, path)
      RUNTIME_PARAM(std::span<const char>, dtype))
```

**Parameters:**

- `path` (string, runtime) - File path
- `dtype` (string, runtime) - Data type ("int16" | "int32" | "float" | "cfloat")

**Behavior:**

- Opens file on first firing (binary write mode, truncates existing)
- Writes one value per firing
- Converts from float input:
  - `int16`, `int32`: Cast to integer type
  - `float`: Direct copy
  - `cfloat`: Write (real, 0.0)
- Flushes after each write

**Error Handling:**

- Returns `ACTOR_ERROR` on file open failure
- Returns `ACTOR_ERROR` on write error
- Returns `ACTOR_ERROR` on unknown dtype

**Limitations:**

- Stateful (one file per pipeline run)
- File not explicitly closed (relies on OS cleanup)

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | binwrite("output.bin", "float")
}
```

**Use Cases:**

- Data logging
- Result storage
- File-based testing

---

## 8. Rate Conversion Actors

Actors that change signal rate.

### 8.1 decimate(N)

**Signature:**

```cpp
ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N))
```

**Parameters:**

- `N` (int, compile-time) - Decimation factor

**Behavior:**

- Consumes N tokens, outputs first token
- Rate reduction by factor of N
- No filtering (simple downsampling)

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(1.0) | delay(10, 0.0) | decimate(10) | stdout()
}
```

**Use Cases:**

- Rate reduction
- Downsampling
- Data compression

**Note:** No anti-aliasing filter. For proper downsampling, use `fir()` before `decimate()`.

---

## 9. Sink Actors

Actors that output data with no downstream connection.

### 9.1 stdout()

**Signature:**

```cpp
ACTOR(stdout, IN(float, 1), OUT(void, 0))
```

**Behavior:**

- Writes one float value per line to standard output
- Format: `%f\n` (default precision)

**Error Handling:**

- Always returns `ACTOR_OK`
- No explicit error checking on printf

**Example:**

```pdl
clock 1kHz t {
    constant(42.0) | stdout()
}
```

**Output:**

```
42.000000
```

---

### 9.2 stderr()

**Signature:**

```cpp
ACTOR(stderr, IN(float, 1), OUT(void, 0))
```

**Behavior:**

- Writes one float value per line to standard error
- Format: `%f\n` (default precision)

**Error Handling:**

- Always returns `ACTOR_OK`

**Example:**

```pdl
clock 1kHz t {
    constant(1.5) | :signal
    :signal | abs() | stderr()  # Monitor on stderr
    :signal | stdout()          # Output on stdout
}
```

**Use Cases:**

- Error reporting
- Monitoring values
- Separate logging stream

---

### 9.3 stdout_fmt(format)

**Signature:**

```cpp
ACTOR(stdout_fmt, IN(float, 1), OUT(void, 0),
      RUNTIME_PARAM(std::span<const char>, format))
```

**Parameters:**

- `format` (string, runtime) - Output format ("default" | "hex" | "scientific")

**Behavior:**

- Writes float to stdout with specified format:
  - `"default"`: `%.6f\n`
  - `"hex"`: `0x%08x\n` (raw bytes)
  - `"scientific"`: `%.6e\n`

**Error Handling:**

- Always returns `ACTOR_OK`
- Unknown format defaults to "default"

**Example:**

```pdl
clock 1kHz t {
    constant(1234.5) | stdout_fmt("scientific")  # Output: 1.234500e+03
}
```

**Use Cases:**

- Hexadecimal debugging
- Scientific notation output
- Custom formatting

---

## 10. Error Handling Philosophy

### 10.1 Quiet Failures (NaN)

Some actors return NaN instead of ACTOR_ERROR:

- `div()` - Division by zero
- `sqrt()` - Negative input

**Rationale:**

- Allows pipeline to continue running
- Downstream actors can detect and handle NaN
- Consistent with IEEE 754 semantics
- Useful for debugging (NaN propagates through pipeline)

### 10.2 Hard Failures (ACTOR_ERROR)

Actors return ACTOR_ERROR for:

- Invalid parameters (`fft()` with non-power-of-2)
- File I/O errors (open, read, write failures)
- Input parsing errors (`stdin()` parse failure)

**Behavior on ACTOR_ERROR:**

- Pipeline execution halts immediately
- Non-zero exit code
- Error message to stderr (if applicable)

---

## 11. Testing

All actors have:

1. **Compilation tests** - Verify actor compiles in inline PDL
2. **Runtime tests** - C++ unit tests validate behavior
3. **Edge case tests** - NaN, inf, zero, negative, large values

**Test Coverage:**

- 32 compilation tests (codegen_compile.rs)
- 58 runtime tests (5 C++ test suites via CMake)
- 311 total tests in v0.1.2

**Test Report:** `doc/test-report.md`

---

## 12. Usage Patterns

### 12.1 Signal Chain

```pdl
clock 1kHz t {
    constant(1.0) | mul(2.5) | abs() | sqrt() | stdout()
}
```

### 12.2 Frequency Analysis

```pdl
clock 1kHz t {
    binread("signal.bin", "float")
    | delay(256, 0.0)
    | fft(256)
    | mag()
    | stdout()
}
```

### 12.3 Statistics

```pdl
clock 1kHz t {
    stdin()
    | delay(100, 0.0)
    | :signal | mean(100) | :avg
    :avg | stdout()
    :signal | sub(:avg) | rms(100) | stderr()  # Standard deviation approx
}
```

### 12.4 Feedback Loop

```pdl
clock 1kHz t {
    constant(1.0)
    | :input | add(:feedback)
    | :output | mul(0.9)
    | delay(1, 0.0)
    | :feedback

    :output | stdout()
}
```

---

## 13. Future Extensions

**Not in v0.1.2 (see TODO.md for roadmap):**

- IIR filters (biquad, Butterworth, etc.)
- WAV file I/O
- Inverse FFT (ifft)
- Window functions (Hann, Hamming, Blackman)
- Resampling (interpolation, decimation with filtering)
- Advanced statistics (variance, correlation)
- Control flow actors (gate, clipper, limiter)

---

## 14. Compliance

### 14.1 ACTOR Macro Requirements

All actors must:

- Follow `ACTOR(name, IN(...), OUT(...), ...)` format
- Return `ACTOR_OK` (0) or `ACTOR_ERROR` (1)
- Use `struct Actor_<name>` naming convention
- Provide `operator()` for firing

### 14.2 Registry Scanner Compatibility

- Text-level scanning (not C++ parsing)
- Parameters must be space-separated on same line
- No macro variations allowed
- All 25 actors successfully extracted by registry scanner

---

## 15. Changelog

**v0.1.2 (2026-02-15):**

- Initial standard library release
- 25 actors across 8 categories
- Complete test coverage (compilation + runtime)
- Documentation and examples

---

## Appendix A: Quick Reference

| Actor | Input | Output | Purpose |
|-------|-------|--------|---------|
| `constant(v)` | void | float[1] | Constant signal |
| `stdin()` | void | float[1] | Read from stdin |
| `fft(N)` | float[N] | cfloat[N] | FFT transform |
| `c2r()` | cfloat[1] | float[1] | Complex magnitude |
| `mag()` | cfloat[1] | float[1] | Complex magnitude |
| `fir(N,c)` | float[N] | float[1] | FIR filter |
| `mul(g)` | float[1] | float[1] | Multiply by gain |
| `add()` | float[2] | float[1] | Addition |
| `sub()` | float[2] | float[1] | Subtraction |
| `div()` | float[2] | float[1] | Division |
| `abs()` | float[1] | float[1] | Absolute value |
| `sqrt()` | float[1] | float[1] | Square root |
| `threshold(v)` | float[1] | int32[1] | Threshold detector |
| `mean(N)` | float[N] | float[1] | Mean over window |
| `rms(N)` | float[N] | float[1] | RMS over window |
| `min(N)` | float[N] | float[1] | Minimum in window |
| `max(N)` | float[N] | float[1] | Maximum in window |
| `delay(N,i)` | float[1] | float[1] | Feedback delay |
| `binread(p,t)` | void | float[1] | Binary file read |
| `binwrite(p,t)` | float[1] | void | Binary file write |
| `decimate(N)` | float[N] | float[1] | Downsample by N |
| `stdout()` | float[1] | void | Write to stdout |
| `stderr()` | float[1] | void | Write to stderr |
| `stdout_fmt(f)` | float[1] | void | Formatted stdout |
