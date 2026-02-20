#pragma once
/// @file std_math.h
/// @brief Pipit Standard Math Actor Library
///
/// Basic arithmetic and mathematical actors for signal processing.
/// Part of the Pipit runtime library.

#include <cmath>
#include <limits>
#include <pipit.h>

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

/// @}
