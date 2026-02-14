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

// ── Actor registration macros ───────────────────────────────────────────────
//
// ACTOR(name, IN(type, count), OUT(type, count), [PARAM|RUNTIME_PARAM]...)
//
// IN/OUT expand to (type, count) pairs — consumed by the compiler scanner.
// PARAM/RUNTIME_PARAM declare member variables in the actor struct.
// The actor body becomes the operator() of the generated struct.

#define IN(type, count) type, count
#define OUT(type, count) type, count
#define PARAM(type, name) type name;
#define RUNTIME_PARAM(type, name) type name;

#define ACTOR(name, in_spec, out_spec, ...)                                                        \
    struct Actor_##name {                                                                          \
        __VA_ARGS__                                                                                \
        int operator()(const void *_in, void *_out)

// TODO: Runtime scheduler, ring buffer, timer APIs
