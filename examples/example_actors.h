#pragma once
//
// example_actors.h — Example/Demo Actors
//
// Example-specific actors for demonstrations and testing.
// Not part of the standard library.
//

#include <complex>
#include <cstdio>
#include <pipit.h>
#include <span>

// ── Signal processing actors (examples) ──

// ── correlate: Simple correlation (example) ──
//
// Sums 64 input samples (placeholder for actual correlation).
//
// Example: correlate()
//
ACTOR(correlate, IN(float, 64), OUT(float, 1)) {
    float sum = 0;
    for (int i = 0; i < 64; ++i)
        sum += in[i];
    out[0] = sum;
    return ACTOR_OK;
}
}
;

// ── detect: Threshold detector (example) ──
//
// Outputs 1 if input > 0.5, otherwise 0.
//
// Example: detect()
//
ACTOR(detect, IN(float, 1), OUT(int32, 1)) {
    out[0] = (in[0] > 0.5f) ? 1 : 0;
    return ACTOR_OK;
}
}
;

// ── sync_process: Signal processing example ──
//
// Sums 256 input samples (example processing).
//
// Example: sync_process()
//
ACTOR(sync_process, IN(float, 256), OUT(float, 1)) {
    float sum = 0;
    for (int i = 0; i < 256; ++i)
        sum += in[i];
    out[0] = sum;
    return ACTOR_OK;
}
}
;

// ── Sink actors (examples) ──

// ── csvwrite: CSV file writer (placeholder) ──
//
// Placeholder for CSV file writing (does nothing in this version).
// Use binwrite() from std_actors.h for actual file I/O.
//
// Example: csvwrite("output.csv")
//
ACTOR(csvwrite, IN(float, 1), OUT(void, 0), PARAM(std::span<const char>, path)) {
    (void)path;
    (void)in;
    (void)out;
    return ACTOR_OK;
}
}
;
