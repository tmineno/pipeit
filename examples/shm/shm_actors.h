#pragma once
//
// shm_actors.h — Actors for the SHM example
//
// Actors that consume/produce 256-sample blocks to match the SHM
// ring's slot size (1024 bytes = 256 floats).
//

#include <cstdio>
#include <pipit.h>

// ── dump_block: Print 256 samples per firing ──
//
// Prints every Nth sample to stdout (1 line per token by default would
// flood the terminal at 48 kHz).  Prints first and last sample of each
// block for verification.
//
// Example: dump_block()
//
ACTOR(dump_block, IN(float, 256), OUT(void, 0)) {
    // Print first and last sample of each 256-sample block
    std::printf("%.6f ... %.6f\n", static_cast<double>(in[0]), static_cast<double>(in[255]));
    (void)out;
    return ACTOR_OK;
}
}
;
