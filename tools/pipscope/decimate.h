#pragma once
/// @file decimate.h
/// @brief Min/max envelope decimation for waveform rendering

#include <algorithm>
#include <cstddef>

namespace pipscope {

/// Min/max envelope decimation. For every group of `factor` input samples,
/// emit 2 output points (min at bucket start, max at bucket end).
/// Returns number of output samples written.
/// Caller must ensure out_x and out_y have capacity >= 2 * ceil(n / factor).
inline size_t decimate_minmax(const float *in, size_t n, int factor, float *out_x, float *out_y,
                              double dt) {
    if (factor <= 1)
        return 0;
    size_t out_idx = 0;
    for (size_t i = 0; i < n; i += static_cast<size_t>(factor)) {
        size_t end = std::min(i + static_cast<size_t>(factor), n);
        float vmin = in[i], vmax = in[i];
        size_t imin = i, imax = i;
        for (size_t j = i + 1; j < end; j++) {
            if (in[j] < vmin) {
                vmin = in[j];
                imin = j;
            }
            if (in[j] > vmax) {
                vmax = in[j];
                imax = j;
            }
        }
        // Emit in chronological order to preserve waveform shape
        if (imin <= imax) {
            out_x[out_idx] = static_cast<float>(imin * dt);
            out_y[out_idx] = vmin;
            out_idx++;
            out_x[out_idx] = static_cast<float>(imax * dt);
            out_y[out_idx] = vmax;
            out_idx++;
        } else {
            out_x[out_idx] = static_cast<float>(imax * dt);
            out_y[out_idx] = vmax;
            out_idx++;
            out_x[out_idx] = static_cast<float>(imin * dt);
            out_y[out_idx] = vmin;
            out_idx++;
        }
    }
    return out_idx;
}

} // namespace pipscope
