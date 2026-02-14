#include <complex>
#include <pipit.h>
#include <span>

// ── Source actors ──

ACTOR(adc, IN(void, 0), OUT(float, 1), PARAM(int, channel)) {
    (void)in;
    out[0] = 0.0f; // placeholder: hw_adc_read(channel)
    return ACTOR_OK;
}
}
;

// ── Transform actors ──

ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) {
    for (int i = 0; i < N; ++i)
        out[i] = cfloat(in[i], 0.0f);
    return ACTOR_OK;
}
}
;

ACTOR(c2r, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) {
    out[0] = std::abs(in[0]);
    return ACTOR_OK;
}
}
;

ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N) PARAM(std::span<const float>, coeff)) {
    float y = 0;
    for (int i = 0; i < N; i++)
        y += coeff[i] * in[i];
    out[0] = y;
    return ACTOR_OK;
}
}
;

ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) {
    out[0] = in[0] * gain;
    return ACTOR_OK;
}
}
;

ACTOR(add, IN(float, 2), OUT(float, 1)) {
    out[0] = in[0] + in[1];
    return ACTOR_OK;
}
}
;

// ── Feedback actors (§5.10) ──

ACTOR(delay, IN(float, 1), OUT(float, 1), PARAM(int, N) PARAM(float, init)) {
    // Built-in: delay(N, init) provides N initial tokens
    (void)N;
    (void)init;
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

// ── Rate conversion actors ──

ACTOR(decimate, IN(float, N), OUT(float, 1), PARAM(int, N)) {
    out[0] = in[0];
    return ACTOR_OK;
}
}
;

// ── Signal processing actors ──

ACTOR(correlate, IN(float, 64), OUT(float, 1)) {
    float sum = 0;
    for (int i = 0; i < 64; ++i)
        sum += in[i];
    out[0] = sum;
    return ACTOR_OK;
}
}
;

ACTOR(detect, IN(float, 1), OUT(int32, 1)) {
    out[0] = (in[0] > 0.5f) ? 1 : 0;
    return ACTOR_OK;
}
}
;

ACTOR(sync_process, IN(float, 256), OUT(float, 1)) {
    float sum = 0;
    for (int i = 0; i < 256; ++i)
        sum += in[i];
    out[0] = sum;
    return ACTOR_OK;
}
}
;

// ── Sink actors ──

ACTOR(csvwrite, IN(float, 1), OUT(void, 0), PARAM(std::span<const char>, path)) {
    (void)path;
    (void)in;
    (void)out;
    return ACTOR_OK;
}
}
;

ACTOR(stdout, IN(float, 1), OUT(void, 0)) {
    printf("%f\n", in[0]);
    (void)out;
    return ACTOR_OK;
}
}
;
