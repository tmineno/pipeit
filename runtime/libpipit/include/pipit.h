#pragma once

// pipit.h — Pipit runtime library
//
// This header provides the ACTOR macro and runtime API for Pipit pipelines.
// Placeholder for v0.1.0 development.

#include <complex>
#include <cstdint>
#include <span>

// Actor return codes
constexpr int ACTOR_OK = 0;
constexpr int ACTOR_ERROR = 1;

// Type aliases used in actor definitions
using cfloat = std::complex<float>;
using cdouble = std::complex<double>;
using int32 = std::int32_t;

// TODO: ACTOR, PARAM, RUNTIME_PARAM macros
// TODO: Runtime scheduler, ring buffer, timer APIs
