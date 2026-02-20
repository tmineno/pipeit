# Pipit Standard Library Reference

<!-- Auto-generated from std_actors.h by scripts/gen-stdlib-doc.py -->
<!-- Do not edit manually -->

## Quick Reference

| Actor | Input | Output | Description |
|-------|-------|--------|-------------|
| `constant` | void | float[N] | Constant signal source |
| `sine` | void | float[N] | Sine wave generator |
| `square` | void | float[N] | Square wave generator |
| `sawtooth` | void | float[N] | Sawtooth wave generator |
| `triangle` | void | float[N] | Triangle wave generator |
| `noise` | void | float[N] | White noise generator |
| `impulse` | void | float[N] | Impulse train generator |
| `fft` | float[N] | cfloat[N] | Fast Fourier Transform |
| `c2r` | cfloat[N] | float[N] | Complex to Real conversion |
| `mag` | cfloat[SHAPE(N)] | float[SHAPE(N)] | Complex magnitude |
| `fir` | T[N] | T[1] | Finite Impulse Response filter |
| `mul` | T[N] | T[N] | Multiplication |
| `add` | T[2] | T[1] | Addition |
| `sub` | T[2] | T[1] | Subtraction |
| `div` | T[2] | T[1] | Division |
| `abs` | T[1] | T[1] | Absolute value |
| `sqrt` | T[1] | T[1] | Square root |
| `threshold` | T[1] | int32[1] | Threshold detector |
| `mean` | T[N] | T[1] | Running mean |
| `rms` | T[N] | T[1] | Root Mean Square |
| `min` | T[N] | T[1] | Minimum value |
| `max` | T[N] | T[1] | Maximum value |
| `delay` | T[1] | T[1] | Feedback delay |
| `binread` | void | float[1] | Binary file reader |
| `binwrite` | float[1] | void | Binary file writer |
| `decimate` | T[N] | T[1] | Downsampling |
| `stdout` | float[1] | void | Standard output |
| `stderr` | float[1] | void | Standard error output |
| `stdin` | void | float[1] | Standard input |
| `stdout_fmt` | float[1] | void | Formatted standard output |

## Source Actors

### constant

**Constant signal source** — Generates a constant signal value. Useful for testing, DC signals, and gain/offset applications.

**Signature:**

```cpp
ACTOR(constant, IN(void, 0), OUT(float, N), RUNTIME_PARAM(float, value) PARAM(int, N))
```

**Parameters:**

- `value` - Constant output value (runtime parameter)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { constant(1.0) | stdout() }
```

---

### sine

**Sine wave generator** — Generates a sinusoidal signal: `amp * sin(2 * pi * freq * t)`. Time is derived from the task clock via pipit_iteration_index() and pipit_task_rate_hz(), ensuring phase continuity across firings.

**Signature:**

```cpp
ACTOR(sine, IN(void, 0), OUT(float, N), PARAM(float, freq) PARAM(float, amp) PARAM(int, N))
```

**Parameters:**

- `freq` - Frequency in Hz
- `amp` - Peak amplitude

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 48kHz audio { sine(440.0, 1.0) | stdout() }
```

---

### square

**Square wave generator** — Generates a square wave with 50% duty cycle: +amp for the first half of each period, -amp for the second half.

**Signature:**

```cpp
ACTOR(square, IN(void, 0), OUT(float, N), PARAM(float, freq) PARAM(float, amp) PARAM(int, N))
```

**Parameters:**

- `freq` - Frequency in Hz
- `amp` - Peak amplitude

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { square(100.0, 1.0) | stdout() }
```

---

### sawtooth

**Sawtooth wave generator** — Generates a sawtooth wave that ramps linearly from -amp to +amp over each period.

**Signature:**

```cpp
ACTOR(sawtooth, IN(void, 0), OUT(float, N), PARAM(float, freq) PARAM(float, amp) PARAM(int, N))
```

**Parameters:**

- `freq` - Frequency in Hz
- `amp` - Peak amplitude

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { sawtooth(100.0, 1.0) | stdout() }
```

---

### triangle

**Triangle wave generator** — Generates a triangle wave that ramps linearly from -amp to +amp and back over each period.

**Signature:**

```cpp
ACTOR(triangle, IN(void, 0), OUT(float, N), PARAM(float, freq) PARAM(float, amp) PARAM(int, N))
```

**Parameters:**

- `freq` - Frequency in Hz
- `amp` - Peak amplitude

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { triangle(100.0, 1.0) | stdout() }
```

---

### noise

**White noise generator** — Generates uniformly distributed pseudo-random noise in the range [-amp, +amp] using a fast xorshift32 PRNG. Deterministic for a given sequence of firings (state persists across calls).

**Signature:**

```cpp
ACTOR(noise, IN(void, 0), OUT(float, N), PARAM(float, amp) PARAM(int, N))
```

**Parameters:**

- `amp` - Peak amplitude

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { noise(0.5) | stdout() }
```

---

### impulse

**Impulse train generator** — Generates a periodic impulse: outputs 1.0 every `period` samples and 0.0 otherwise. Uses pipit_iteration_index() for sample position.

**Signature:**

```cpp
ACTOR(impulse, IN(void, 0), OUT(float, N), PARAM(int, period) PARAM(int, N))
```

**Parameters:**

- `period` - Impulse period in samples (must be > 0)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { impulse(100) | stdout() }
```

---

## Transform Actors

### fft

**Fast Fourier Transform** — Computes FFT using Cooley-Tukey algorithm (radix-2, DIT). Requires N to be a power of 2.

**Signature:**

```cpp
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N))
```

**Parameters:**

- `N` - FFT size (must be power of 2)

**Returns:** ACTOR_OK on success, ACTOR_ERROR if N is not a power of 2

**Example:**

```pdl
fft(256)
```

---

### c2r

**Complex to Real conversion** — Converts complex signal to real by taking magnitude.

**Signature:**

```cpp
ACTOR(c2r, IN(cfloat, N), OUT(float, N), PARAM(int, N))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
c2r()
```

---

### mag

**Complex magnitude** — Computes magnitude of complex signal (same as c2r).

**Signature:**

```cpp
ACTOR(mag, IN(cfloat, SHAPE(N)), OUT(float, SHAPE(N)), PARAM(int, N))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
mag()
```

---

### fir

**Finite Impulse Response filter** — Applies FIR filter with given coefficients. Polymorphic: works with float and double wire types.

**Signature:**

```cpp
ACTOR(fir, IN(T, N), OUT(T, 1), PARAM(std::span<const T>, coeff) PARAM(int, N))
```

**Parameters:**

- `coeff` - Filter coefficients
- `N` - Filter length (must match coefficient array size)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
fir([0.1, 0.2, 0.4, 0.2, 0.1])
```

---

## Basic Arithmetic Actors

### mul

**Multiplication** — Multiplies signal by a runtime-adjustable gain. Polymorphic: works with any numeric wire type (float, double, etc.).

**Signature:**

```cpp
template <typename T> ACTOR(mul, IN(T, N), OUT(T, N), RUNTIME_PARAM(T, gain) PARAM(int, N))
```

**Parameters:**

- `gain` - Multiplication factor (runtime parameter)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
mul($gain)
mul(2.5)
```

---

### add

**Addition** — Adds two signals together. Polymorphic: works with any numeric wire type.

**Signature:**

```cpp
template <typename T> ACTOR(add, IN(T, 2), OUT(T, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | add(:b)
```

---

### sub

**Subtraction** — Subtracts second input from first (out = in[0] - in[1]). Polymorphic: works with any numeric wire type.

**Signature:**

```cpp
template <typename T> ACTOR(sub, IN(T, 2), OUT(T, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | sub(:b)
```

---

### div

**Division** — Divides first input by second (out = in[0] / in[1]). Returns NaN on division by zero for floating-point types (IEEE 754). Returns zero on division by zero for integer types. Polymorphic: works with any numeric wire type.

**Signature:**

```cpp
template <typename T> ACTOR(div, IN(T, 2), OUT(T, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | div(:b)
```

---

### abs

**Absolute value** — Computes absolute value of signal. Polymorphic: works with any numeric wire type.

**Signature:**

```cpp
template <typename T> ACTOR(abs, IN(T, 1), OUT(T, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
abs()
```

---

### sqrt

**Square root** — Computes square root of signal. Returns NaN for negative inputs (IEEE 754 behavior). Polymorphic: works with float and double wire types.

**Signature:**

```cpp
template <typename T> ACTOR(sqrt, IN(T, 1), OUT(T, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
sqrt()
```

---

### threshold

**Threshold detector** — Converts signal to int32 based on threshold. Outputs 1 if input > threshold, otherwise 0. Useful for control signals in modal tasks. Polymorphic input: works with any comparable wire type.

**Signature:**

```cpp
template <typename T> ACTOR(threshold, IN(T, 1), OUT(int32, 1), RUNTIME_PARAM(T, value))
```

**Parameters:**

- `value` - Threshold value (runtime parameter)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
threshold(0.5)
```

---

## Statistics Actors

### mean

**Running mean** — Computes mean (average) over N samples. Consumes N tokens, outputs 1 token. Polymorphic: works with any numeric wire type.

**Signature:**

```cpp
template <typename T> ACTOR(mean, IN(T, N), OUT(T, 1), PARAM(int, N))
```

**Parameters:**

- `N` - Number of samples to average

**Returns:** ACTOR_OK on success

**Example:**

```pdl
mean(10)
```

---

### rms

**Root Mean Square** — Computes RMS over N samples. Consumes N tokens, outputs 1 token. Polymorphic: works with float and double wire types.

**Signature:**

```cpp
template <typename T> ACTOR(rms, IN(T, N), OUT(T, 1), PARAM(int, N))
```

**Parameters:**

- `N` - Number of samples for RMS calculation

**Returns:** ACTOR_OK on success

**Example:**

```pdl
rms(10)
```

---

### min

**Minimum value** — Finds minimum value over N samples. Consumes N tokens, outputs 1 token. Polymorphic: works with any comparable wire type.

**Signature:**

```cpp
template <typename T> ACTOR(min, IN(T, N), OUT(T, 1), PARAM(int, N))
```

**Parameters:**

- `N` - Number of samples to search

**Returns:** ACTOR_OK on success

**Example:**

```pdl
min(10)
```

---

### max

**Maximum value** — Finds maximum value over N samples. Consumes N tokens, outputs 1 token. Polymorphic: works with any comparable wire type.

**Signature:**

```cpp
template <typename T> ACTOR(max, IN(T, N), OUT(T, 1), PARAM(int, N))
```

**Parameters:**

- `N` - Number of samples to search

**Returns:** ACTOR_OK on success

**Example:**

```pdl
max(10)
```

---

## Feedback Actors

### delay

**Feedback delay** — Provides initial tokens for feedback loops. Built-in support: delay(N, init) provides N initial tokens. Polymorphic: works with any wire type.

**Signature:**

```cpp
template <typename T> ACTOR(delay, IN(T, 1), OUT(T, 1), PARAM(int, N) PARAM(T, init))
```

**Parameters:**

- `N` - Number of initial tokens to provide
- `init` - Initial value for tokens

**Returns:** ACTOR_OK on success

**Example:**

```pdl
delay(1, 0.0)
```

---

## File I/O Actors

### binread

**Binary file reader** — Reads binary data from file and converts to float output. Opens file on first firing, returns ACTOR_ERROR on EOF or read error. Stateful actor (one file per pipeline run). Supported dtypes: "int16", "int32", "float", "cfloat" For cfloat, outputs the magnitude as float.

**Signature:**

```cpp
ACTOR(binread, IN(void, 0), OUT(float, 1), RUNTIME_PARAM(std::span<const char>, path) RUNTIME_PARAM(std::span<const char>, dtype))
```

**Parameters:**

- `path` - File path (runtime parameter)
- `dtype` - Data type: "int16", "int32", "float", or "cfloat" (runtime parameter)

**Returns:** ACTOR_OK on success, ACTOR_ERROR on EOF or read error

**Example:**

```pdl
binread("data.bin", "int16")
```

---

### binwrite

**Binary file writer** — Writes binary data to file from float input. Opens file on first firing, returns ACTOR_ERROR on write error. Stateful actor (one file per pipeline run). Supported dtypes: "int16", "int32", "float", "cfloat" For cfloat, writes (real, 0) complex number.

**Signature:**

```cpp
ACTOR(binwrite, IN(float, 1), OUT(void, 0), RUNTIME_PARAM(std::span<const char>, path) RUNTIME_PARAM(std::span<const char>, dtype))
```

**Parameters:**

- `path` - File path (runtime parameter)
- `dtype` - Data type: "int16", "int32", "float", or "cfloat" (runtime parameter)

**Returns:** ACTOR_OK on success, ACTOR_ERROR on write error

**Example:**

```pdl
binwrite("output.bin", "float")
```

---

## Rate Conversion Actors

### decimate

**Downsampling** — Consumes N tokens, outputs first token (rate reduction by N). Polymorphic: works with any wire type.

**Signature:**

```cpp
template <typename T> ACTOR(decimate, IN(T, N), OUT(T, 1), PARAM(int, N))
```

**Parameters:**

- `N` - Decimation factor

**Returns:** ACTOR_OK on success

**Example:**

```pdl
decimate(10)
```

---

## Sink Actors

### stdout

**Standard output** — Writes signal values to stdout (one per line).

**Signature:**

```cpp
ACTOR(stdout, IN(float, 1), OUT(void, 0))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
stdout()
```

---

### stderr

**Standard error output** — Writes signal values to stderr (one per line). Useful for error reporting and monitoring.

**Signature:**

```cpp
ACTOR(stderr, IN(float, 1), OUT(void, 0))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
stderr()
```

---

### stdin

**Standard input** — Reads signal values from stdin (one per line). Returns ACTOR_ERROR on EOF or parse failure.

**Signature:**

```cpp
ACTOR(stdin, IN(void, 0), OUT(float, 1))
```

**Returns:** ACTOR_OK on success, ACTOR_ERROR on EOF or parse failure

**Example:**

```pdl
stdin()
```

---

### stdout_fmt

**Formatted standard output** — Writes signal values to stdout with custom formatting. Formats: "default" (%.6f), "hex" (raw bytes), "scientific" (%.6e)

**Signature:**

```cpp
ACTOR(stdout_fmt, IN(float, 1), OUT(void, 0), RUNTIME_PARAM(std::span<const char>, format))
```

**Parameters:**

- `format` - Output format: "default", "hex", or "scientific" (runtime parameter)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
stdout_fmt("hex")
```

---
