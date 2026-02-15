#pragma once
//
// std_actors.h — Pipit Standard Actor Library
//
// Standard library of actors for common signal processing tasks.
// Part of the Pipit runtime library.
//

#include <cmath>
#include <complex>
#include <cstdio>
#include <limits>
#include <pipit.h>
#include <span>

// ── Source actors ──

// ── constant: Constant signal source ──
//
// Generates a constant signal value.
// Useful for testing, DC signals, and gain/offset applications.
//
// Example: constant(1.0)
//
ACTOR(constant, IN(void, 0), OUT(float, 1), RUNTIME_PARAM(float, value)) {
    (void)in;
    out[0] = value;
    return ACTOR_OK;
}
}
;

// ── Transform actors ──

// ── fft: Fast Fourier Transform ──
//
// Computes FFT using Cooley-Tukey algorithm (radix-2, DIT).
// Requires N to be a power of 2.
//
// Example: fft(256)
//
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    // Verify N is power of 2
    if (N <= 0 || (N & (N - 1)) != 0) {
        return ACTOR_ERROR;
    }

    // Copy input to output (convert real to complex)
    for (int i = 0; i < N; ++i) {
        out[i] = cfloat(in[i], 0.0f);
    }

    // Bit-reversal permutation
    int bits = 0;
    int temp = N;
    while (temp > 1) {
        bits++;
        temp >>= 1;
    }

    for (int i = 0; i < N; ++i) {
        int j = 0;
        for (int b = 0; b < bits; ++b) {
            if (i & (1 << b)) {
                j |= 1 << (bits - 1 - b);
            }
        }
        if (j > i) {
            cfloat tmp = out[i];
            out[i] = out[j];
            out[j] = tmp;
        }
    }

    // Cooley-Tukey FFT (iterative, decimation-in-time)
    const float PI = 3.14159265358979323846f;
    for (int len = 2; len <= N; len *= 2) {
        float angle = -2.0f * PI / len;
        cfloat wlen(std::cos(angle), std::sin(angle));

        for (int i = 0; i < N; i += len) {
            cfloat w(1.0f, 0.0f);
            for (int j = 0; j < len / 2; ++j) {
                cfloat u = out[i + j];
                cfloat v = out[i + j + len / 2] * w;
                out[i + j] = u + v;
                out[i + j + len / 2] = u - v;
                w *= wlen;
            }
        }
    }

    return ACTOR_OK;
}
}
;

// ── c2r: Complex to Real conversion ──
//
// Converts complex signal to real by taking magnitude.
//
// Example: c2r()
//
ACTOR(c2r, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

// ── mag: Complex magnitude ──
//
// Computes magnitude of complex signal (same as c2r).
//
// Example: mag()
//
ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

// ── fir: Finite Impulse Response filter ──
//
// Applies FIR filter with given coefficients.
//
// Example: fir([0.1, 0.2, 0.4, 0.2, 0.1])
//
ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N) PARAM(std::span<const float>, coeff)) {
    float y = 0;
    for (int i = 0; i < N; i++)
        y += coeff[i] * in[i];
    out[0] = y;
    return ACTOR_OK;
}
}
;

// ── Basic arithmetic actors ──

// ── mul: Multiplication ──
//
// Multiplies signal by a runtime-adjustable gain.
//
// Example: mul($gain) or mul(2.5)
//
ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) {
    out[0] = in[0] * gain;
    return ACTOR_OK;
}
}
;

// ── add: Addition ──
//
// Adds two signals together.
//
// Example: :a | add(:b)
//
ACTOR(add, IN(float, 2), OUT(float, 1)) {
    out[0] = in[0] + in[1];
    return ACTOR_OK;
}
}
;

// ── sub: Subtraction ──
//
// Subtracts second input from first (out = in[0] - in[1]).
//
// Example: :a | sub(:b)
//
ACTOR(sub, IN(float, 2), OUT(float, 1)) {
    out[0] = in[0] - in[1];
    return ACTOR_OK;
}
}
;

// ── div: Division ──
//
// Divides first input by second (out = in[0] / in[1]).
// Returns NaN on division by zero (IEEE 754 behavior).
//
// Example: :a | div(:b)
//
ACTOR(div, IN(float, 2), OUT(float, 1)) {
    if (in[1] == 0.0f) {
        out[0] = std::numeric_limits<float>::quiet_NaN();
    } else {
        out[0] = in[0] / in[1];
    }
    return ACTOR_OK;
}
}
;

// ── abs: Absolute value ──
//
// Computes absolute value of signal.
//
// Example: abs()
//
ACTOR(abs, IN(float, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

// ── sqrt: Square root ──
//
// Computes square root of signal.
// Returns NaN for negative inputs (IEEE 754 behavior).
//
// Example: sqrt()
//
ACTOR(sqrt, IN(float, 1), OUT(float, 1)) {
    out[0] = std::sqrt(in[0]);
    return ACTOR_OK;
}
}
;

// ── threshold: Threshold detector ──
//
// Converts float to int32 based on threshold.
// Outputs 1 if input > threshold, otherwise 0.
// Useful for control signals in modal tasks.
//
// Example: threshold(0.5)
//
ACTOR(threshold, IN(float, 1), OUT(int32, 1), RUNTIME_PARAM(float, value)) {
    out[0] = (in[0] > value) ? 1 : 0;
    return ACTOR_OK;
}
}
;

// ── Statistics actors ──

// ── mean: Running mean ──
//
// Computes mean (average) over N samples.
// Consumes N tokens, outputs 1 token.
//
// Example: mean(10)
//
ACTOR(mean, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    float sum = 0.0f;
    for (int i = 0; i < N; ++i) {
        sum += in[i];
    }
    out[0] = sum / N;
    return ACTOR_OK;
}
}
;

// ── rms: Root Mean Square ──
//
// Computes RMS over N samples.
// Consumes N tokens, outputs 1 token.
//
// Example: rms(10)
//
ACTOR(rms, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    float sum_sq = 0.0f;
    for (int i = 0; i < N; ++i) {
        sum_sq += in[i] * in[i];
    }
    out[0] = std::sqrt(sum_sq / N);
    return ACTOR_OK;
}
}
;

// ── min: Minimum value ──
//
// Finds minimum value over N samples.
// Consumes N tokens, outputs 1 token.
//
// Example: min(10)
//
ACTOR(min, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    float min_val = in[0];
    for (int i = 1; i < N; ++i) {
        if (in[i] < min_val) {
            min_val = in[i];
        }
    }
    out[0] = min_val;
    return ACTOR_OK;
}
}
;

// ── max: Maximum value ──
//
// Finds maximum value over N samples.
// Consumes N tokens, outputs 1 token.
//
// Example: max(10)
//
ACTOR(max, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    float max_val = in[0];
    for (int i = 1; i < N; ++i) {
        if (in[i] > max_val) {
            max_val = in[i];
        }
    }
    out[0] = max_val;
    return ACTOR_OK;
}
}
;

// ── Feedback actors ──

// ── delay: Feedback delay ──
//
// Provides initial tokens for feedback loops.
// Built-in support: delay(N, init) provides N initial tokens.
//
// Example: delay(1, 0.0)
//
ACTOR(delay, IN(float, 1), OUT(float, 1), PARAM(int, N) PARAM(float, init)) {
    // Built-in: delay(N, init) provides N initial tokens
    (void)N;
    (void)init;
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

// ── File I/O actors ──

// ── binread: Binary file reader ──
//
// Reads binary data from file and converts to float output.
// Opens file on first firing, returns ACTOR_ERROR on EOF or read error.
// Stateful actor (one file per pipeline run).
//
// Supported dtypes: "int16", "int32", "float", "cfloat"
// For cfloat, outputs the magnitude as float.
//
// Example: binread("data.bin", "int16")
//
ACTOR(binread, IN(void, 0), OUT(float, 1),
      RUNTIME_PARAM(std::span<const char>, path) RUNTIME_PARAM(std::span<const char>, dtype)) {
    (void)in;
    static FILE *fp = nullptr;
    static bool initialized = false;

    if (!initialized) {
        std::string path_str(path.data(), path.size());
        fp = fopen(path_str.c_str(), "rb");
        if (!fp) {
            return ACTOR_ERROR;
        }
        initialized = true;
    }

    std::string dtype_str(dtype.data(), dtype.size());
    if (dtype_str == "float") {
        float val;
        if (fread(&val, sizeof(float), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
        out[0] = val;
    } else if (dtype_str == "int16") {
        int16_t val;
        if (fread(&val, sizeof(int16_t), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
        out[0] = static_cast<float>(val);
    } else if (dtype_str == "int32") {
        int32_t val;
        if (fread(&val, sizeof(int32_t), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
        out[0] = static_cast<float>(val);
    } else if (dtype_str == "cfloat") {
        cfloat val;
        if (fread(&val, sizeof(cfloat), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
        out[0] = std::abs(val);
    } else {
        return ACTOR_ERROR; // Unknown dtype
    }

    return ACTOR_OK;
}
}
;

// ── binwrite: Binary file writer ──
//
// Writes binary data to file from float input.
// Opens file on first firing, returns ACTOR_ERROR on write error.
// Stateful actor (one file per pipeline run).
//
// Supported dtypes: "int16", "int32", "float", "cfloat"
// For cfloat, writes (real, 0) complex number.
//
// Example: binwrite("output.bin", "float")
//
ACTOR(binwrite, IN(float, 1), OUT(void, 0),
      RUNTIME_PARAM(std::span<const char>, path) RUNTIME_PARAM(std::span<const char>, dtype)) {
    (void)out;
    static FILE *fp = nullptr;
    static bool initialized = false;

    if (!initialized) {
        std::string path_str(path.data(), path.size());
        fp = fopen(path_str.c_str(), "wb");
        if (!fp) {
            return ACTOR_ERROR;
        }
        initialized = true;
    }

    std::string dtype_str(dtype.data(), dtype.size());
    if (dtype_str == "float") {
        float val = in[0];
        if (fwrite(&val, sizeof(float), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
    } else if (dtype_str == "int16") {
        int16_t val = static_cast<int16_t>(in[0]);
        if (fwrite(&val, sizeof(int16_t), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
    } else if (dtype_str == "int32") {
        int32_t val = static_cast<int32_t>(in[0]);
        if (fwrite(&val, sizeof(int32_t), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
    } else if (dtype_str == "cfloat") {
        cfloat val(in[0], 0.0f);
        if (fwrite(&val, sizeof(cfloat), 1, fp) != 1) {
            return ACTOR_ERROR;
        }
    } else {
        return ACTOR_ERROR; // Unknown dtype
    }

    // Flush to ensure data is written
    fflush(fp);
    return ACTOR_OK;
}
}
;

// ── Rate conversion actors ──

// ── decimate: Downsampling ──
//
// Consumes N tokens, outputs first token (rate reduction by N).
//
// Example: decimate(10)
//
ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

// ── Sink actors ──

// ── stdout: Standard output ──
//
// Writes signal values to stdout (one per line).
//
// Example: stdout()
//
ACTOR(stdout, IN(float, 1), OUT(void, 0)) {
    printf("%f\n", in[0]);
    (void)out;
    return ACTOR_OK;
}
}
;

// ── stderr: Standard error output ──
//
// Writes signal values to stderr (one per line).
// Useful for error reporting and monitoring.
//
// Example: stderr()
//
ACTOR(stderr, IN(float, 1), OUT(void, 0)) {
    fprintf(stderr, "%f\n", in[0]);
    (void)out;
    return ACTOR_OK;
}
}
;

// ── stdin: Standard input ──
//
// Reads signal values from stdin (one per line).
// Returns ACTOR_ERROR on EOF or parse failure.
//
// Example: stdin()
//
ACTOR(stdin, IN(void, 0), OUT(float, 1)) {
    (void)in;
    float value;
    if (scanf("%f", &value) != 1) {
        return ACTOR_ERROR;
    }
    out[0] = value;
    return ACTOR_OK;
}
}
;

// ── stdout_fmt: Formatted standard output ──
//
// Writes signal values to stdout with custom formatting.
// Formats: "default" (%.6f), "hex" (raw bytes), "scientific" (%.6e)
//
// Example: stdout_fmt("hex")
//
ACTOR(stdout_fmt, IN(float, 1), OUT(void, 0), RUNTIME_PARAM(std::span<const char>, format)) {
    std::string fmt(format.data(), format.size());
    if (fmt == "hex") {
        printf("0x%08x\n", *reinterpret_cast<const uint32_t *>(&in[0]));
    } else if (fmt == "scientific") {
        printf("%.6e\n", in[0]);
    } else { // default
        printf("%.6f\n", in[0]);
    }
    (void)out;
    return ACTOR_OK;
}
}
;
