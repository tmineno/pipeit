# Pipit Standard Library Reference

<!-- Auto-generated from std_actors.h by scripts/gen-stdlib-doc.py -->
<!-- Do not edit manually -->

## Quick Reference

| Actor | Input | Output | Description |
|-------|-------|--------|-------------|
| `constant` | void | float[1] | Constant signal source |
| `fft` | float[N] | cfloat[N] | Fast Fourier Transform |
| `c2r` | cfloat[1] | float[1] | Complex to Real conversion |
| `fir` | float[N] | float[1] | Complex magnitude |
| `mul` | float[1] | float[1] | Multiplication |
| `add` | float[2] | float[1] | Addition |
| `sub` | float[2] | float[1] | Subtraction |
| `div` | float[2] | float[1] | Division |
| `abs` | float[1] | float[1] | Absolute value |
| `sqrt` | float[1] | float[1] | Square root |
| `threshold` | float[1] | int32[1] | Threshold detector |
| `mean` | float[N] | float[1] | Running mean |
| `rms` | float[N] | float[1] | Root Mean Square |
| `min` | float[N] | float[1] | Minimum value |
| `max` | float[N] | float[1] | Maximum value |
| `delay` | float[1] | float[1] | Feedback delay |
| `binread` | void | float[1] | Binary file reader |
| `binwrite` | float[1] | void | Binary file writer |
| `decimate` | float[N] | float[1] | Downsampling |
| `stdout` | float[1] | void | Standard output |
| `stderr` | float[1] | void | Standard error output |
| `stdin` | void | float[1] | Standard input |
| `stdout_fmt` | float[1] | void | Formatted standard output |

## Source Actors

### constant

**Constant signal source** — Generates a constant signal value. Useful for testing, DC signals, and gain/offset applications.

**Signature:**

```cpp
ACTOR(constant, IN(void, 0), OUT(float, 1), RUNTIME_PARAM(float, value))
```

**Parameters:**

- `value` - Constant output value (runtime parameter)

**Returns:** ACTOR_OK on success

**Example:**

```pdl
clock 1kHz t { constant(1.0) | stdout() }
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
ACTOR(c2r, IN(cfloat, 1), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
c2r()
```

---

### fir

**Complex magnitude** — Computes magnitude of complex signal (same as c2r).

**Signature:**

```cpp
ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N) PARAM(std::span<const float>, coeff))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
mag()
```

---

## Basic Arithmetic Actors

### mul

**Multiplication** — Multiplies signal by a runtime-adjustable gain.

**Signature:**

```cpp
ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain))
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

**Addition** — Adds two signals together.

**Signature:**

```cpp
ACTOR(add, IN(float, 2), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | add(:b)
```

---

### sub

**Subtraction** — Subtracts second input from first (out = in[0] - in[1]).

**Signature:**

```cpp
ACTOR(sub, IN(float, 2), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | sub(:b)
```

---

### div

**Division** — Divides first input by second (out = in[0] / in[1]). Returns NaN on division by zero (IEEE 754 behavior).

**Signature:**

```cpp
ACTOR(div, IN(float, 2), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
:a | div(:b)
```

---

### abs

**Absolute value** — Computes absolute value of signal.

**Signature:**

```cpp
ACTOR(abs, IN(float, 1), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
abs()
```

---

### sqrt

**Square root** — Computes square root of signal. Returns NaN for negative inputs (IEEE 754 behavior).

**Signature:**

```cpp
ACTOR(sqrt, IN(float, 1), OUT(float, 1))
```

**Returns:** ACTOR_OK on success

**Example:**

```pdl
sqrt()
```

---

### threshold

**Threshold detector** — Converts float to int32 based on threshold. Outputs 1 if input > threshold, otherwise 0. Useful for control signals in modal tasks.

**Signature:**

```cpp
ACTOR(threshold, IN(float, 1), OUT(int32, 1), RUNTIME_PARAM(float, value))
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

**Running mean** — Computes mean (average) over N samples. Consumes N tokens, outputs 1 token.

**Signature:**

```cpp
ACTOR(mean, IN(float, N), OUT(float, 1), PARAM(int, N))
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

**Root Mean Square** — Computes RMS over N samples. Consumes N tokens, outputs 1 token.

**Signature:**

```cpp
ACTOR(rms, IN(float, N), OUT(float, 1), PARAM(int, N))
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

**Minimum value** — Finds minimum value over N samples. Consumes N tokens, outputs 1 token.

**Signature:**

```cpp
ACTOR(min, IN(float, N), OUT(float, 1), PARAM(int, N))
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

**Maximum value** — Finds maximum value over N samples. Consumes N tokens, outputs 1 token.

**Signature:**

```cpp
ACTOR(max, IN(float, N), OUT(float, 1), PARAM(int, N))
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

**Feedback delay** — Provides initial tokens for feedback loops. Built-in support: delay(N, init) provides N initial tokens.

**Signature:**

```cpp
ACTOR(delay, IN(float, 1), OUT(float, 1), PARAM(int, N) PARAM(float, init))
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

**Downsampling** — Consumes N tokens, outputs first token (rate reduction by N).

**Signature:**

```cpp
ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N))
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
