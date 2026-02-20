#pragma once
/// @file std_math.h
/// @brief Pipit Standard Math Actor Library
///
/// Basic arithmetic and mathematical actors for signal processing.
/// Part of the Pipit runtime library.

#include <cmath>
#include <limits>
#include <pipit.h>
#include <xsimd/xsimd.hpp>

/// @defgroup arithmetic_actors Basic Arithmetic Actors
/// @{

/// @brief Multiplication
///
/// Multiplies signal by a runtime-adjustable gain.
/// Polymorphic: works with any numeric wire type (float, double, etc.).
///
/// @param gain Multiplication factor (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// mul($gain)
/// mul(2.5)
/// @endcode
template <typename T> ACTOR(mul, IN(T, N), OUT(T, N), RUNTIME_PARAM(T, gain) PARAM(int, N)) {
    using batch = xsimd::batch<T>;
    constexpr int S = static_cast<int>(batch::size);
    auto vgain = batch(gain);
    int i = 0;
    for (; i + S <= N; i += S) {
        auto v = batch::load_unaligned(&in[i]);
        (v * vgain).store_unaligned(&out[i]);
    }
    for (; i < N; ++i)
        out[i] = in[i] * gain;
    return ACTOR_OK;
}
}
;

/// @brief Addition
///
/// Adds two signals together.
/// Polymorphic: works with any numeric wire type.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | add(:b)
/// @endcode
template <typename T> ACTOR(add, IN(T, 2), OUT(T, 1)) {
    out[0] = in[0] + in[1];
    return ACTOR_OK;
}
}
;

/// @brief Subtraction
///
/// Subtracts second input from first (out = in[0] - in[1]).
/// Polymorphic: works with any numeric wire type.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | sub(:b)
/// @endcode
template <typename T> ACTOR(sub, IN(T, 2), OUT(T, 1)) {
    out[0] = in[0] - in[1];
    return ACTOR_OK;
}
}
;

/// @brief Division
///
/// Divides first input by second (out = in[0] / in[1]).
/// Returns NaN on division by zero for floating-point types (IEEE 754).
/// Returns zero on division by zero for integer types.
/// Polymorphic: works with any numeric wire type.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// :a | div(:b)
/// @endcode
template <typename T> ACTOR(div, IN(T, 2), OUT(T, 1)) {
    if (in[1] == T{}) {
        out[0] = std::numeric_limits<T>::quiet_NaN();
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
/// Polymorphic: works with any numeric wire type.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// abs()
/// @endcode
template <typename T> ACTOR(abs, IN(T, 1), OUT(T, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

/// @brief Square root
///
/// Computes square root of signal.
/// Returns NaN for negative inputs (IEEE 754 behavior).
/// Polymorphic: works with float and double wire types.
///
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// sqrt()
/// @endcode
template <typename T> ACTOR(sqrt, IN(T, 1), OUT(T, 1)) {
    out[0] = std::sqrt(in[0]);
    return ACTOR_OK;
}
}
;

/// @brief Threshold detector
///
/// Converts signal to int32 based on threshold.
/// Outputs 1 if input > threshold, otherwise 0.
/// Useful for control signals in modal tasks.
/// Polymorphic input: works with any comparable wire type.
///
/// @param value Threshold value (runtime parameter)
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// threshold(0.5)
/// @endcode
template <typename T> ACTOR(threshold, IN(T, 1), OUT(int32, 1), RUNTIME_PARAM(T, value)) {
    out[0] = (in[0] > value) ? 1 : 0;
    return ACTOR_OK;
}
}
;

/// @brief Convolution
///
/// Applies discrete convolution of input signal with a kernel.
/// Produces N output samples (1:1 rate), unlike fir which is N:1.
/// Uses causal zero-padded convolution: out[i] = sum_j kernel[j] * in[i-j].
/// Polymorphic: works with float and double wire types.
///
/// Preconditions: kernel.size() > 0.
/// Postconditions: out[0..N-1] contains the convolved signal.
/// Failure modes: None (always returns ACTOR_OK).
/// Side effects: None.
///
/// @param kernel Convolution kernel coefficients
/// @return ACTOR_OK on success
///
/// Example usage:
/// @code{.pdl}
/// convolve([0.2, 0.6, 0.2])
/// @endcode
template <typename T>
ACTOR(convolve, IN(T, N), OUT(T, N), PARAM(std::span<const T>, kernel) PARAM(int, N)) {
    const int M = static_cast<int>(kernel.size());
    // Partial-kernel region (boundary): out[i] where i < M-1
    int i = 0;
    for (; i < M - 1 && i < N; ++i) {
        T sum = T{};
        for (int j = 0; j <= i; ++j)
            sum += kernel[j] * in[i - j];
        out[i] = sum;
    }
    // Full-kernel region: no bounds check needed
    for (; i < N; ++i) {
        T sum = T{};
        for (int j = 0; j < M; ++j)
            sum += kernel[j] * in[i - j];
        out[i] = sum;
    }
    return ACTOR_OK;
}
}
;

/// @}
