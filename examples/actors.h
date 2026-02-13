#include <pipit.h>
#include <span>
#include <complex>

// ── Source actors ──

ACTOR(adc, IN(void, 0), OUT(float, 1), PARAM(int, channel)) {
    out[0] = hw_adc_read(channel);
    return ACTOR_OK;
}

// ── Transform actors ──

ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    fft_exec(in, out, N);
    return ACTOR_OK;
}

ACTOR(c2r, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}

ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}

ACTOR(fir, IN(float, N), OUT(float, 1),
      PARAM(int, N),
      PARAM(std::span<const float>, coeff)) {
    float y = 0;
    for (int i = 0; i < N; i++) y += coeff[i] * in[i];
    out[0] = y;
    return ACTOR_OK;
}

ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) {
    out[0] = in[0] * gain;
    return ACTOR_OK;
}

// ── Rate conversion actors ──

ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    out[0] = in[0];
    return ACTOR_OK;
}

// ── Signal processing actors ──

ACTOR(correlate, IN(float, 64), OUT(float, 1)) {
    out[0] = sync_correlate(in, 64);
    return ACTOR_OK;
}

ACTOR(detect, IN(float, 1), OUT(int32, 1)) {
    out[0] = (in[0] > 0.5f) ? 1 : 0;
    return ACTOR_OK;
}

ACTOR(sync_process, IN(float, 256), OUT(float, 1)) {
    out[0] = sync_demod(in, 256);
    return ACTOR_OK;
}

// ── Sink actors ──

ACTOR(csvwrite, IN(float, 1), OUT(void, 0), PARAM(std::span<const char>, path)) {
    csv_append(path.data(), in[0]);
    return ACTOR_OK;
}

ACTOR(stdout, IN(float, 1), OUT(void, 0)) {
    printf("%f\n", in[0]);
    return ACTOR_OK;
}
