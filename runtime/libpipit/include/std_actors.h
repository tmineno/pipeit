#pragma once
/// @file std_actors.h
/// @brief Pipit Standard Actor Library
///
/// Standard library of actors for common signal processing tasks.
/// Part of the Pipit runtime library.

#include <cmath>
#include <complex>
#include <cstdio>
#include <limits>
#include <pipit.h>
#include <span>

/// @defgroup source_actors Source Actors
/// @{

/// @brief Constant signal source
///
/// Generates a constant signal value.
/// Useful for testing, DC signals, and gain/offset applications.
///
/// @param value Constant output value (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// clock 1kHz t { constant(1.0) | stdout() }
/// @endcode
ACTOR(constant, IN(void, 0), OUT(float, N), RUNTIME_PARAM(float, value) PARAM(int, N)) {
    (void)in;
    for (int i = 0; i < N; ++i) {
        out[i] = value;
    }
    return ACTOR_OK;
}
}
;

/// @}

/// @defgroup transform_actors Transform Actors
/// @{

/// @brief Fast Fourier Transform
///
/// Computes FFT using Cooley-Tukey algorithm (radix-2, DIT).
/// Requires N to be a power of 2.
///
/// @param N FFT size (must be power of 2)
/// @return ACTOR_OK on success, ACTOR_ERROR if N is not a power of 2
///
/// Example usage:
/// @code{.pdl}
/// fft(256)
/// @endcode
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

/// @brief Complex to Real conversion
///
/// Converts complex signal to real by taking magnitude.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// c2r()
/// @endcode
ACTOR(c2r, IN(cfloat, N), OUT(float, N), PARAM(int, N)) {
    for (int i = 0; i < N; ++i) {
        out[i] = std::abs(in[i]);
    }
    return ACTOR_OK;
}
}
;

/// @brief Complex magnitude
///
/// Computes magnitude of complex signal (same as c2r).
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// mag()
/// @endcode
ACTOR(mag, IN(cfloat, SHAPE(N)), OUT(float, SHAPE(N)), PARAM(int, N)) {
    for (int i = 0; i < N; ++i) {
        out[i] = std::abs(in[i]);
    }
    return ACTOR_OK;
}
}
;

/// @brief Finite Impulse Response filter
///
/// Applies FIR filter with given coefficients.
///
/// @param coeff Filter coefficients
/// @param N Filter length (must match coefficient array size)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// fir([0.1, 0.2, 0.4, 0.2, 0.1])
/// @endcode
ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(std::span<const float>, coeff) PARAM(int, N)) {
    float y = 0;
    for (int i = 0; i < N; i++)
        y += coeff[i] * in[i];
    out[0] = y;
    return ACTOR_OK;
}
}
;

/// @}

/// @defgroup arithmetic_actors Basic Arithmetic Actors
/// @{

/// @brief Multiplication
///
/// Multiplies signal by a runtime-adjustable gain.
///
/// @param gain Multiplication factor (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// mul($gain)
/// mul(2.5)
/// @endcode
ACTOR(mul, IN(float, N), OUT(float, N), RUNTIME_PARAM(float, gain) PARAM(int, N)) {
    for (int i = 0; i < N; ++i) {
        out[i] = in[i] * gain;
    }
    return ACTOR_OK;
}
}
;

/// @brief Addition
///
/// Adds two signals together.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | add(:b)
/// @endcode
ACTOR(add, IN(float, 2), OUT(float, 1)) {
    out[0] = in[0] + in[1];
    return ACTOR_OK;
}
}
;

/// @brief Subtraction
///
/// Subtracts second input from first (out = in[0] - in[1]).
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | sub(:b)
/// @endcode
ACTOR(sub, IN(float, 2), OUT(float, 1)) {
    out[0] = in[0] - in[1];
    return ACTOR_OK;
}
}
;

/// @brief Division
///
/// Divides first input by second (out = in[0] / in[1]).
/// Returns NaN on division by zero (IEEE 754 behavior).
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | div(:b)
/// @endcode
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

/// @brief Absolute value
///
/// Computes absolute value of signal.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// abs()
/// @endcode
ACTOR(abs, IN(float, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

/// @brief Square root
///
/// Computes square root of signal.
/// Returns NaN for negative inputs (IEEE 754 behavior).
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// sqrt()
/// @endcode
ACTOR(sqrt, IN(float, 1), OUT(float, 1)) {
    out[0] = std::sqrt(in[0]);
    return ACTOR_OK;
}
}
;

/// @brief Threshold detector
///
/// Converts float to int32 based on threshold.
/// Outputs 1 if input > threshold, otherwise 0.
/// Useful for control signals in modal tasks.
///
/// @param value Threshold value (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// threshold(0.5)
/// @endcode
ACTOR(threshold, IN(float, 1), OUT(int32, 1), RUNTIME_PARAM(float, value)) {
    out[0] = (in[0] > value) ? 1 : 0;
    return ACTOR_OK;
}
}
;

/// @}

/// @defgroup statistics_actors Statistics Actors
/// @{

/// @brief Running mean
///
/// Computes mean (average) over N samples.
/// Consumes N tokens, outputs 1 token.
///
/// @param N Number of samples to average
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// mean(10)
/// @endcode
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

/// @brief Root Mean Square
///
/// Computes RMS over N samples.
/// Consumes N tokens, outputs 1 token.
///
/// @param N Number of samples for RMS calculation
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// rms(10)
/// @endcode
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

/// @brief Minimum value
///
/// Finds minimum value over N samples.
/// Consumes N tokens, outputs 1 token.
///
/// @param N Number of samples to search
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// min(10)
/// @endcode
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

/// @brief Maximum value
///
/// Finds maximum value over N samples.
/// Consumes N tokens, outputs 1 token.
///
/// @param N Number of samples to search
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// max(10)
/// @endcode
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

/// @}

/// @defgroup feedback_actors Feedback Actors
/// @{

/// @brief Feedback delay
///
/// Provides initial tokens for feedback loops.
/// Built-in support: delay(N, init) provides N initial tokens.
///
/// @param N Number of initial tokens to provide
/// @param init Initial value for tokens
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// delay(1, 0.0)
/// @endcode
ACTOR(delay, IN(float, 1), OUT(float, 1), PARAM(int, N) PARAM(float, init)) {
    // Built-in: delay(N, init) provides N initial tokens
    (void)N;
    (void)init;
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

/// @}

/// @defgroup fileio_actors File I/O Actors
/// @{

/// @brief Binary file reader
///
/// Reads binary data from file and converts to float output.
/// Opens file on first firing, returns ACTOR_ERROR on EOF or read error.
/// Stateful actor (one file per pipeline run).
///
/// Supported dtypes: "int16", "int32", "float", "cfloat"
/// For cfloat, outputs the magnitude as float.
///
/// @param path File path (runtime parameter)
/// @param dtype Data type: "int16", "int32", "float", or "cfloat" (runtime parameter)
/// @return ACTOR_OK on success, ACTOR_ERROR on EOF or read error
///
/// Example usage:
/// @code{.pdl}
/// binread("data.bin", "int16")
/// @endcode
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

/// @brief Binary file writer
///
/// Writes binary data to file from float input.
/// Opens file on first firing, returns ACTOR_ERROR on write error.
/// Stateful actor (one file per pipeline run).
///
/// Supported dtypes: "int16", "int32", "float", "cfloat"
/// For cfloat, writes (real, 0) complex number.
///
/// @param path File path (runtime parameter)
/// @param dtype Data type: "int16", "int32", "float", or "cfloat" (runtime parameter)
/// @return ACTOR_OK on success, ACTOR_ERROR on write error
///
/// Example usage:
/// @code{.pdl}
/// binwrite("output.bin", "float")
/// @endcode
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

/// @}

/// @defgroup rate_conversion_actors Rate Conversion Actors
/// @{

/// @brief Downsampling
///
/// Consumes N tokens, outputs first token (rate reduction by N).
///
/// @param N Decimation factor
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// decimate(10)
/// @endcode
ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

/// @}

/// @defgroup sink_actors Sink Actors
/// @{

/// @brief Standard output
///
/// Writes signal values to stdout (one per line).
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// stdout()
/// @endcode
ACTOR(stdout, IN(float, 1), OUT(void, 0)) {
    printf("%f\n", in[0]);
    (void)out;
    return ACTOR_OK;
}
}
;

/// @brief Standard error output
///
/// Writes signal values to stderr (one per line).
/// Useful for error reporting and monitoring.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// stderr()
/// @endcode
ACTOR(stderr, IN(float, 1), OUT(void, 0)) {
    fprintf(stderr, "%f\n", in[0]);
    (void)out;
    return ACTOR_OK;
}
}
;

/// @brief Standard input
///
/// Reads signal values from stdin (one per line).
/// Returns ACTOR_ERROR on EOF or parse failure.
///
/// @return ACTOR_OK on success, ACTOR_ERROR on EOF or parse failure
///
/// Example usage:
/// @code{.pdl}
/// stdin()
/// @endcode
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

/// @brief Formatted standard output
///
/// Writes signal values to stdout with custom formatting.
/// Formats: "default" (%.6f), "hex" (raw bytes), "scientific" (%.6e)
///
/// @param format Output format: "default", "hex", or "scientific" (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// stdout_fmt("hex")
/// @endcode
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

/// @}
