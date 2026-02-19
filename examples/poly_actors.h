#pragma once

// poly_actors.h — Example polymorphic actors for v0.3.0 testing
//
// These actors use `template <typename T>` before the ACTOR macro,
// allowing a single definition to operate on multiple wire types.

#include "pipit.h"

// ── Polymorphic scale: multiply input by a gain factor ──────────────────────

template <typename T> ACTOR(poly_scale, IN(T, 1), OUT(T, 1), PARAM(T, gain)) {
    out[0] = in[0] * gain;
    return ACTOR_OK;
}
}
;

// ── Polymorphic pass: identity (pass-through) ───────────────────────────────

template <typename T> ACTOR(poly_pass, IN(T, 1), OUT(T, 1)) {
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

// ── Polymorphic block pass: identity for N-sample blocks ────────────────────

template <typename T> ACTOR(poly_block_pass, IN(T, N), OUT(T, N), PARAM(int, N)) {
    for (int i = 0; i < N; ++i)
        out[i] = in[i];
    return ACTOR_OK;
}
}
;

// ── Polymorphic accumulate: running sum ─────────────────────────────────────

template <typename T> ACTOR(poly_accum, IN(T, 1), OUT(T, 1)) {
    static T sum{};
    sum = sum + in[0];
    out[0] = sum;
    return ACTOR_OK;
}
}
;
