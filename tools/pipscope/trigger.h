//
// trigger.h — Oscilloscope trigger logic for pipscope
//
// Pure functions and data types for amplitude-based edge triggering.
// No GUI or receiver dependencies — testable in isolation.
//

#pragma once
#include <cstddef>
#include <cstdint>

namespace pipscope {

struct TriggerConfig {
    bool enabled = false;
    float level = 0.0f;
    enum Edge { Rising, Falling } edge = Rising;
    enum Mode { Auto, Normal } mode = Auto;
    uint16_t source_chan_id = 0; // chan_id (not index) for stable identity
    bool waiting = false;        // true when Normal mode has no trigger (UI state)
};

/// Find the most recent trigger event in samples[0..n).
///
/// Searches backward from the end of the valid range so the returned index
/// corresponds to the *latest* crossing that still leaves enough room for
/// a display window of [pre_margin, post_margin] samples around it.
///
/// Returns the sample index of the crossing, or -1 if none found.
inline int find_trigger(const float *samples, size_t n, float level, TriggerConfig::Edge edge,
                        int pre_margin, int post_margin) {
    int start = pre_margin;
    int end = static_cast<int>(n) - post_margin;
    if (start < 1)
        start = 1; // need samples[i-1]
    if (end <= start)
        return -1;

    for (int i = end - 1; i >= start; --i) {
        if (edge == TriggerConfig::Rising && samples[i - 1] < level && samples[i] >= level)
            return i;
        if (edge == TriggerConfig::Falling && samples[i - 1] > level && samples[i] <= level)
            return i;
    }
    return -1;
}

} // namespace pipscope
