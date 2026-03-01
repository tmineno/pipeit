//
// pipscope — Oscilloscope GUI for Pipit PPKT/PSHM streams
//
// Receives PPKT packets over UDP and/or reads PSHM shared memory rings,
// displaying real-time waveforms using ImGui + ImPlot.
//
// Usage: pipscope [--port <port>] [--address <addr>] [--shm <name>]
//        pipscope [-p <port>] [-a <addr>]
//

#include <algorithm>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include <GL/gl.h>
#include <GLFW/glfw3.h>
#include <imgui.h>
#include <imgui_impl_glfw.h>
#include <imgui_impl_opengl3.h>
#include <implot.h>

#include "cli.h"
#include "decimate.h"
#include "ppkt_receiver.h"
#include "shm_receiver.h"
#include "trigger.h"

// ── Signal handling ──────────────────────────────────────────────────────────

static volatile sig_atomic_t g_shutdown = 0;

static void signal_handler(int) { g_shutdown = 1; }

// ── Channel visibility types ─────────────────────────────────────────────────

enum class SourceKind : uint8_t { Socket, Shm };

struct ChannelKey {
    SourceKind kind;
    uint16_t chan_id;
    bool operator<(const ChannelKey &o) const {
        if (kind != o.kind)
            return kind < o.kind;
        return chan_id < o.chan_id;
    }
};

/// Normalize SHM name to bare form (strip all leading '/').
/// probe_shm() adds the POSIX '/' prefix internally at shm_open time.
static std::string normalize_shm_name(const char *name) {
    const char *p = name;
    while (*p == '/')
        ++p;
    return std::string(p);
}

// ── SI-prefix formatting ─────────────────────────────────────────────────────

/// Format a value with SI prefix (p/n/u/m/ /K/M/G/T).
/// Returns pointer to thread_local buffer — copy before calling again.
static const char *si_format(double val, const char *unit) {
    thread_local char buf[64];
    struct Prefix {
        double scale;
        const char *sym;
    };
    static constexpr Prefix table[] = {
        {1e12, "T"}, {1e9, "G"},  {1e6, "M"},  {1e3, "K"},   {1.0, ""},
        {1e-3, "m"}, {1e-6, "u"}, {1e-9, "n"}, {1e-12, "p"},
    };
    double a = val < 0 ? -val : val;
    if (a < 1e-15 && a > -1e-15) {
        snprintf(buf, sizeof(buf), "0 %s", unit);
        return buf;
    }
    for (const auto &p : table) {
        if (a >= p.scale * 0.9995) {
            snprintf(buf, sizeof(buf), "%.3g %s%s", val / p.scale, p.sym, unit);
            return buf;
        }
    }
    snprintf(buf, sizeof(buf), "%.3g p%s", val / 1e-12, unit);
    return buf;
}

/// ImPlot axis formatter for SI-prefixed time values.
static int si_time_formatter(double value, char *buff, int size, void *) {
    const char *s = si_format(value, "s");
    return snprintf(buff, size, "%s", s);
}

/// ImPlot axis formatter for SI-prefixed amplitude values.
static int si_amplitude_formatter(double value, char *buff, int size, void *) {
    const char *s = si_format(value, "");
    return snprintf(buff, size, "%s", s);
}

// ── CLI parsing (implemented in cli.h) ───────────────────────────────────────

static bool is_wsl2() {
    FILE *f = fopen("/proc/version", "r");
    if (!f)
        return false;
    char buf[256] = {};
    fread(buf, 1, sizeof(buf) - 1, f);
    fclose(f);
    return strstr(buf, "microsoft") != nullptr || strstr(buf, "Microsoft") != nullptr;
}

static GLFWwindow *try_create_window() {
    glfwWindowHint(GLFW_CONTEXT_VERSION_MAJOR, 3);
    glfwWindowHint(GLFW_CONTEXT_VERSION_MINOR, 3);
    glfwWindowHint(GLFW_OPENGL_PROFILE, GLFW_OPENGL_CORE_PROFILE);
    return glfwCreateWindow(1280, 720, "pipscope", nullptr, nullptr);
}

static bool init_window(GLFWwindow *&window, bool vsync = false) {
    // WSL2/WSLg: GLFW 3.4 prefers Wayland when WAYLAND_DISPLAY is set,
    // but WSLg Wayland can produce invisible windows. Force X11.
    if (is_wsl2())
        glfwInitHint(GLFW_PLATFORM, GLFW_PLATFORM_X11);

    if (!glfwInit()) {
        fprintf(stderr, "Error: failed to initialize GLFW.\n"
                        "Is a display server running (WSLg/X11/Wayland)?\n");
        return false;
    }

    window = try_create_window();
    if (!window) {
        // GPU driver may be unavailable (e.g. WSL2 without d3d12 passthrough).
        // Retry with Mesa software rendering.
        fprintf(stderr, "Warning: GPU context failed, falling back to software rendering.\n");
        setenv("LIBGL_ALWAYS_SOFTWARE", "1", 1);
        glfwTerminate();
        if (is_wsl2())
            glfwInitHint(GLFW_PLATFORM, GLFW_PLATFORM_X11);
        if (!glfwInit()) {
            fprintf(stderr, "Error: failed to re-initialize GLFW for software rendering.\n");
            return false;
        }
        window = try_create_window();
        if (!window) {
            fprintf(stderr,
                    "Error: failed to create GLFW window (even with software rendering).\n");
            glfwTerminate();
            return false;
        }
    }

    glfwMakeContextCurrent(window);
    bool wsl = is_wsl2();
    bool force_no_vsync = false;
    if (const char *no_vsync_env = getenv("PIPSCOPE_NO_VSYNC")) {
        force_no_vsync = strcmp(no_vsync_env, "1") == 0 || strcmp(no_vsync_env, "true") == 0 ||
                         strcmp(no_vsync_env, "TRUE") == 0;
    }
    bool use_vsync = (vsync || wsl) && !force_no_vsync;
    glfwSwapInterval(use_vsync ? 1 : 0);
    if (wsl && !vsync && use_vsync) {
        fprintf(stderr, "pipscope: WSL detected; enabling vsync by default "
                        "(set PIPSCOPE_NO_VSYNC=1 to disable)\n");
    }

    // WSLg can place windows at garbage coordinates (e.g. -32730,-32709).
    // Reposition to a sane default if off-screen.
    int wx, wy;
    glfwGetWindowPos(window, &wx, &wy);
    if (wx < -10000 || wy < -10000 || wx > 10000 || wy > 10000)
        glfwSetWindowPos(window, 100, 100);

    return true;
}

static void shutdown_window(GLFWwindow *window) {
    if (window != nullptr) {
        glfwDestroyWindow(window);
    }
    glfwTerminate();
}

static void init_imgui(GLFWwindow *window) {
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImPlot::CreateContext();

    ImGui::StyleColorsDark();
    ImGui_ImplGlfw_InitForOpenGL(window, true);
    ImGui_ImplOpenGL3_Init("#version 330");
}

static void shutdown_imgui() {
    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplGlfw_Shutdown();
    ImPlot::DestroyContext();
    ImGui::DestroyContext();
}

enum class ConnStatus { Disconnected, Connected, Error };

static constexpr float kUncalDropRate = 0.05f; // 5%

struct AppState {
    bool paused = false;
    bool auto_y = true;
    int display_samples = 8192;
    std::vector<pipscope::ChannelSnapshot> snapshots;

    char address_buf[128] = "0.0.0.0:9100";
    ConnStatus conn_status = ConnStatus::Disconnected;
    char status_msg[256] = "Disconnected";

    pipscope::TriggerConfig trigger;

    // Plot refresh rate tracking (how often waveform data actually updates)
    int update_count = 0;
    double last_rate_time = 0.0;
    float refresh_rate_hz = 0.0f;

    // Persistent raw snapshot buffer (reused across frames to avoid allocation)
    std::vector<pipscope::ChannelSnapshot> raw_snapshots;

    // Receiver metrics (1-second windowed, same cadence as refresh_rate_hz)
    uint64_t prev_recv_packets = 0;
    uint64_t prev_recv_bytes = 0;
    float recv_pps = 0.0f;
    float recv_mbps = 0.0f;
    float snapshot_ms = 0.0f;

    // Snapshot rate limiting
    int snapshot_hz = 0;
    double last_snapshot_time = 0.0;

    size_t buffer_capacity = 1'000'000;

    // SHM receivers (one per --shm argument or runtime add)
    std::vector<std::unique_ptr<pipscope::ShmReceiver>> shm_receivers;

    // Channel manager sub-window
    bool show_channel_manager = false;
    std::set<ChannelKey> hidden_channels;
    int add_source_type = 1; // 0=Socket, 1=SHM
    char add_input_buf[128] = {};
    char channel_mgr_status[256] = {};

    // X-range readback from last rendered frame (for auto display_samples)
    double visible_x_min = 0.0;
    double visible_x_max = 0.0;
    bool has_valid_x_range = false;

    // Freeze Auto-Y on the trigger channel when user is dragging the level
    // line (tracked from previous frame so setup phase can use it).
    bool trigger_y_frozen = false;
};

static void begin_frame() {
    ImGui_ImplOpenGL3_NewFrame();
    ImGui_ImplGlfw_NewFrame();
    ImGui::NewFrame();
}

static void end_frame(GLFWwindow *window) {
    ImGui::Render();
    int display_w = 0;
    int display_h = 0;
    glfwGetFramebufferSize(window, &display_w, &display_h);
    glViewport(0, 0, display_w, display_h);
    glClearColor(0.06f, 0.06f, 0.06f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
    glfwSwapBuffers(window);
}

static void do_connect(AppState &state, pipscope::PpktReceiver &receiver) {
    receiver.stop();
    receiver.clear_channels();
    state.snapshots.clear();
    state.raw_snapshots.clear();

    if (receiver.start(state.address_buf)) {
        state.conn_status = ConnStatus::Connected;
        snprintf(state.status_msg, sizeof(state.status_msg), "Listening on %s", state.address_buf);
    } else {
        state.conn_status = ConnStatus::Error;
        snprintf(state.status_msg, sizeof(state.status_msg), "Failed to bind %s",
                 state.address_buf);
    }
}

static void render_toolbar(AppState &state, pipscope::PpktReceiver &receiver) {
    // ── Row 1: transport, display, metrics ───────────────────────────────
    if (ImGui::Button(state.paused ? "Resume" : "Pause")) {
        state.paused = !state.paused;
    }
    ImGui::SameLine();
    ImGui::Checkbox("Auto-Y", &state.auto_y);
    ImGui::SameLine();
    if (ImGui::Button("Channels")) {
        state.show_channel_manager = !state.show_channel_manager;
    }
    ImGui::SameLine();
    ImGui::Text("| Samples: %s", si_format(static_cast<double>(state.display_samples), ""));

    // Compute refresh rate + receiver metrics over 1-second windows
    {
        double now = ImGui::GetTime();
        double elapsed = now - state.last_rate_time;
        if (elapsed >= 1.0) {
            state.refresh_rate_hz = static_cast<float>(state.update_count / elapsed);
            state.update_count = 0;

            auto m = receiver.metrics();
            uint64_t dpkt = m.recv_packets - state.prev_recv_packets;
            uint64_t dbytes = m.recv_bytes - state.prev_recv_bytes;
            state.recv_pps = static_cast<float>(dpkt / elapsed);
            state.recv_mbps = static_cast<float>(dbytes / elapsed / (1024.0 * 1024.0));
            state.prev_recv_packets = m.recv_packets;
            state.prev_recv_bytes = m.recv_bytes;

            state.last_rate_time = now;
        }
    }
    {
        char refresh_buf[32], pps_buf[32], bps_buf[32], snap_buf[32];
        snprintf(refresh_buf, sizeof(refresh_buf), "%s", si_format(state.refresh_rate_hz, "Hz"));
        snprintf(pps_buf, sizeof(pps_buf), "%s", si_format(state.recv_pps, "pps"));
        snprintf(bps_buf, sizeof(bps_buf), "%s",
                 si_format(state.recv_mbps * 1024.0 * 1024.0, "B/s"));
        snprintf(snap_buf, sizeof(snap_buf), "%s",
                 si_format(static_cast<double>(state.snapshot_ms) / 1000.0, "s"));
        ImGui::SameLine();
        ImGui::Text("| %s | recv: %s  %s | snap: %s", refresh_buf, pps_buf, bps_buf, snap_buf);
    }
    // ── Row 2: trigger controls ──────────────────────────────────────────
    ImGui::Checkbox("Trigger", &state.trigger.enabled);
    ImGui::SameLine();

    if (!state.trigger.enabled)
        ImGui::BeginDisabled();

    ImGui::SetNextItemWidth(100);
    ImGui::InputFloat("Level", &state.trigger.level, 0, 0, "%.3f");
    ImGui::SameLine();

    const char *edge_label =
        state.trigger.edge == pipscope::TriggerConfig::Rising ? "Edge: /" : "Edge: \\";
    if (ImGui::Button(edge_label)) {
        state.trigger.edge = state.trigger.edge == pipscope::TriggerConfig::Rising
                                 ? pipscope::TriggerConfig::Falling
                                 : pipscope::TriggerConfig::Rising;
    }
    ImGui::SameLine();

    const char *mode_label =
        state.trigger.mode == pipscope::TriggerConfig::Auto ? "Mode: Auto" : "Mode: Norm";
    if (ImGui::Button(mode_label)) {
        state.trigger.mode = state.trigger.mode == pipscope::TriggerConfig::Auto
                                 ? pipscope::TriggerConfig::Normal
                                 : pipscope::TriggerConfig::Auto;
    }
    ImGui::SameLine();

    // Source channel combo — populated from raw_snapshots (includes hidden channels).
    // Disabled when no data sources are connected.
    bool no_sources = state.raw_snapshots.empty();
    if (no_sources)
        ImGui::BeginDisabled();
    {
        char combo_label[64];
        const char *src_label = nullptr;
        for (const auto &snap : state.raw_snapshots) {
            if (snap.chan_id == state.trigger.source_chan_id && !snap.label.empty()) {
                src_label = snap.label.c_str();
                break;
            }
        }
        if (src_label)
            snprintf(combo_label, sizeof(combo_label), "%s", src_label);
        else
            snprintf(combo_label, sizeof(combo_label), "Ch %u", state.trigger.source_chan_id);
        ImGui::SetNextItemWidth(120);
        if (ImGui::BeginCombo("Source", combo_label)) {
            for (const auto &snap : state.raw_snapshots) {
                char item_label[64];
                if (!snap.label.empty())
                    snprintf(item_label, sizeof(item_label), "%s", snap.label.c_str());
                else
                    snprintf(item_label, sizeof(item_label), "Ch %u", snap.chan_id);
                bool selected = (snap.chan_id == state.trigger.source_chan_id);
                if (ImGui::Selectable(item_label, selected)) {
                    state.trigger.source_chan_id = snap.chan_id;
                }
                if (selected)
                    ImGui::SetItemDefaultFocus();
            }
            ImGui::EndCombo();
        }
    }
    if (no_sources)
        ImGui::EndDisabled();

    if (!state.trigger.enabled)
        ImGui::EndDisabled();
}

static void extract_window(std::vector<pipscope::ChannelSnapshot> &dst,
                           const std::vector<pipscope::ChannelSnapshot> &src, int offset,
                           int count) {
    dst.resize(src.size());
    for (size_t c = 0; c < src.size(); ++c) {
        dst[c].chan_id = src[c].chan_id;
        dst[c].sample_rate_hz = src[c].sample_rate_hz;
        dst[c].packet_count = src[c].packet_count;
        dst[c].stats = src[c].stats;
        dst[c].label = src[c].label;

        int avail = static_cast<int>(src[c].samples.size());
        int start = offset;
        int end = offset + count;
        if (start < 0)
            start = 0;
        if (end > avail)
            end = avail;
        int n = end - start;
        if (n > 0) {
            dst[c].samples.resize(static_cast<size_t>(n));
            std::memcpy(dst[c].samples.data(), src[c].samples.data() + start,
                        static_cast<size_t>(n) * sizeof(float));
        } else {
            dst[c].samples.clear();
        }
    }
}

static void take_tail(std::vector<pipscope::ChannelSnapshot> &dst,
                      const std::vector<pipscope::ChannelSnapshot> &src, int count) {
    dst.resize(src.size());
    for (size_t c = 0; c < src.size(); ++c) {
        dst[c].chan_id = src[c].chan_id;
        dst[c].sample_rate_hz = src[c].sample_rate_hz;
        dst[c].packet_count = src[c].packet_count;
        dst[c].stats = src[c].stats;
        dst[c].label = src[c].label;

        int avail = static_cast<int>(src[c].samples.size());
        int n = count < avail ? count : avail;
        if (n > 0) {
            dst[c].samples.resize(static_cast<size_t>(n));
            std::memcpy(dst[c].samples.data(), src[c].samples.data() + (avail - n),
                        static_cast<size_t>(n) * sizeof(float));
        } else {
            dst[c].samples.clear();
        }
    }
}

// Returns true if every channel in snaps has at least min_samples samples.
static bool all_channels_have(const std::vector<pipscope::ChannelSnapshot> &snaps,
                              int min_samples) {
    if (snaps.empty())
        return false;
    for (const auto &ch : snaps) {
        if (static_cast<int>(ch.samples.size()) < min_samples)
            return false;
    }
    return true;
}

static void update_snapshots(AppState &state, pipscope::PpktReceiver &receiver) {
    if (state.paused)
        return;

    // Rate-limit snapshots when snapshot_hz is set
    if (state.snapshot_hz > 0) {
        double now = ImGui::GetTime();
        if (now - state.last_snapshot_time < 1.0 / state.snapshot_hz)
            return;
        state.last_snapshot_time = now;
    }

    int ds = state.display_samples;
    size_t request_size =
        state.trigger.enabled ? static_cast<size_t>(ds) * 2 : static_cast<size_t>(ds);

    auto snap_t0 = std::chrono::steady_clock::now();
    receiver.snapshot_into(state.raw_snapshots, request_size);

    // Clear stale labels on PPKT entries (snapshot_into reuses entries that
    // may have held SHM data in a previous frame when the vector was larger)
    for (size_t i = 0; i < state.raw_snapshots.size(); ++i)
        state.raw_snapshots[i].label.clear();

    // Append SHM channel snapshots
    for (const auto &shm_recv : state.shm_receivers) {
        pipscope::ChannelSnapshot shm_snap;
        shm_recv->snapshot_into(shm_snap, request_size);
        state.raw_snapshots.push_back(std::move(shm_snap));
    }

    auto snap_t1 = std::chrono::steady_clock::now();
    state.snapshot_ms = std::chrono::duration<float, std::milli>(snap_t1 - snap_t0).count();

    const auto &raw = state.raw_snapshots;

    if (state.trigger.enabled) {
        // Find trigger source channel by chan_id
        const pipscope::ChannelSnapshot *trig_ch = nullptr;
        for (const auto &ch : raw) {
            if (ch.chan_id == state.trigger.source_chan_id) {
                trig_ch = &ch;
                break;
            }
        }
        if (!trig_ch) {
            state.trigger.waiting = true;
            return; // source channel not present yet
        }

        int pre = ds / 2;
        int post = ds - pre;
        int idx = pipscope::find_trigger(trig_ch->samples.data(), trig_ch->samples.size(),
                                         state.trigger.level, state.trigger.edge, pre, post);

        if (idx >= 0) {
            state.trigger.waiting = false;
            extract_window(state.snapshots, raw, idx - pre, ds);
            state.update_count++;
        } else if (state.trigger.mode == pipscope::TriggerConfig::Auto) {
            state.trigger.waiting = false;
            // Hold previous display if insufficient data (e.g. after inter_frame_gap clear)
            if (!all_channels_have(raw, ds) && !state.snapshots.empty())
                return;
            take_tail(state.snapshots, raw, ds);
            state.update_count++;
        } else {
            // Normal mode: no trigger found — keep previous display
            state.trigger.waiting = true;
        }
    } else {
        state.trigger.waiting = false;
        // Hold previous display if insufficient data (e.g. after inter_frame_gap clear)
        if (!all_channels_have(raw, ds) && !state.snapshots.empty())
            return;
        take_tail(state.snapshots, raw, ds);
        state.update_count++;
    }
}

static void render_channels(AppState &state) {
    int n = static_cast<int>(state.snapshots.size());
    if (n == 0)
        return;

    // X-axis constraint: use buffer capacity (not snapshot size) to avoid a
    // feedback loop where requesting fewer samples shrinks the axis constraint,
    // which shrinks the computed display_samples, locking at the minimum.
    double max_rate = 0;
    for (const auto &ch : state.snapshots) {
        if (ch.sample_rate_hz > max_rate)
            max_rate = ch.sample_rate_hz;
    }
    double max_x_duration =
        max_rate > 0 ? static_cast<double>(state.buffer_capacity) / max_rate : 0.0;

    ImVec2 avail = ImGui::GetContentRegionAvail();

    if (ImPlot::BeginSubplots("##scope", n, 1, avail,
                              ImPlotSubplotFlags_LinkAllX | ImPlotSubplotFlags_NoTitle)) {
        for (int i = 0; i < n; ++i) {
            const auto &channel = state.snapshots[static_cast<size_t>(i)];

            // Use a static title for stable plot ID. Dynamic info (rate, count)
            // is rendered as an overlay inside the plot to avoid ID instability.
            // ImPlot's BeginChild uses the full title for window naming, causing
            // plot re-creation when the display text changes.
            char title[64];
            if (!channel.label.empty())
                snprintf(title, sizeof(title), "%s##ch%u", channel.label.c_str(), channel.chan_id);
            else
                snprintf(title, sizeof(title), "Ch %u##ch%u", channel.chan_id, channel.chan_id);

            if (ImPlot::BeginPlot(title, ImVec2(-1, -1), ImPlotFlags_NoMouseText)) {
                bool is_trigger_chan =
                    state.trigger.enabled && channel.chan_id == state.trigger.source_chan_id;

                // Y-axis: use AutoFit flag when auto-Y is on (and not frozen
                // for trigger drag). This avoids SetupAxisLimits entirely,
                // sidestepping the HasRange/RangeCond persistence bug where
                // ImPlotCond_Always locks the axis across frames even after
                // the call is removed.
                bool do_auto_y = state.auto_y && !channel.samples.empty() &&
                                 !(is_trigger_chan && state.trigger_y_frozen);

                ImPlotAxisFlags x_flags =
                    ((i < n - 1) ? ImPlotAxisFlags_NoTickLabels : 0) | ImPlotAxisFlags_NoHighlight;
                ImPlotAxisFlags y_flags = ImPlotAxisFlags_NoHighlight;
                if (do_auto_y)
                    y_flags |= ImPlotAxisFlags_AutoFit;
                ImPlot::SetupAxes("Time", "Amplitude", x_flags, y_flags);

                // SI-prefix formatter on bottom x-axis
                if (i == n - 1)
                    ImPlot::SetupAxisFormat(ImAxis_X1, si_time_formatter);

                // Fixed-width SI formatter stabilizes label width across frames
                ImPlot::SetupAxisFormat(ImAxis_Y1, si_amplitude_formatter);

                // Constrain x-axis to buffer capacity range
                if (max_x_duration > 0)
                    ImPlot::SetupAxisLimitsConstraints(ImAxis_X1, 0.0, max_x_duration);

                // Plot waveform
                if (!channel.samples.empty()) {
                    double dt = channel.sample_rate_hz > 0 ? 1.0 / channel.sample_rate_hz : 1.0;
                    size_t ns = channel.samples.size();
                    static constexpr size_t kMaxPlotPoints = 4000;

                    if (ns > kMaxPlotPoints) {
                        int factor = static_cast<int>(ns / (kMaxPlotPoints / 2));
                        thread_local std::vector<float> dec_x, dec_y;
                        size_t max_out = 2 * ((ns + factor - 1) / factor);
                        dec_x.resize(max_out);
                        dec_y.resize(max_out);
                        size_t dn = pipscope::decimate_minmax(channel.samples.data(), ns, factor,
                                                              dec_x.data(), dec_y.data(), dt);
                        if (dn > 0) {
                            ImPlot::PlotLine("signal", dec_x.data(), dec_y.data(),
                                             static_cast<int>(dn));
                        }
                    } else {
                        ImPlot::PlotLine("signal", channel.samples.data(), static_cast<int>(ns), dt,
                                         0.0);
                    }
                }

                // Trigger level line (draggable, yellow) on the source channel.
                // Track drag state for next frame's setup phase (one-frame delay
                // avoids calling IsPlotHovered during setup, which would crash).
                if (is_trigger_chan) {
                    double trig_level = static_cast<double>(state.trigger.level);
                    bool dragging = ImPlot::DragLineY(0, &trig_level, ImVec4(1, 1, 0, 1), 2,
                                                      ImPlotDragToolFlags_NoFit);
                    state.trigger.level = static_cast<float>(trig_level);
                    state.trigger_y_frozen = dragging || ImPlot::IsPlotHovered();
                }

                // "Waiting for trigger..." overlay
                if (state.trigger.waiting) {
                    ImPlot::PushPlotClipRect();
                    ImVec2 pos = ImPlot::GetPlotPos();
                    ImVec2 sz = ImPlot::GetPlotSize();
                    ImGui::GetWindowDrawList()->AddText(
                        ImVec2(pos.x + sz.x * 0.5f - 70, pos.y + sz.y * 0.5f - 8),
                        IM_COL32(200, 200, 200, 180), "Waiting for trigger...");
                    ImPlot::PopPlotClipRect();
                }

                // Drop stats overlay (top-left corner)
                if (channel.stats.dropped_frames > 0 || channel.stats.inter_frame_gaps > 0) {
                    ImPlot::PushPlotClipRect();
                    ImVec2 pos = ImPlot::GetPlotPos();
                    char drop_txt[128];
                    int off = 0;
                    if (channel.stats.dropped_frames > 0) {
                        off += snprintf(drop_txt + off, sizeof(drop_txt) - off, "drop:%lu",
                                        static_cast<unsigned long>(channel.stats.dropped_frames));
                    }
                    if (channel.stats.inter_frame_gaps > 0) {
                        if (off > 0)
                            off += snprintf(drop_txt + off, sizeof(drop_txt) - off, " ");
                        snprintf(drop_txt + off, sizeof(drop_txt) - off, "gap:%lu",
                                 static_cast<unsigned long>(channel.stats.inter_frame_gaps));
                    }
                    ImGui::GetWindowDrawList()->AddText(ImVec2(pos.x + 5, pos.y + 5),
                                                        IM_COL32(255, 160, 50, 200), drop_txt);
                    ImPlot::PopPlotClipRect();
                }

                // UNCAL overlay — drop rate or inter-frame gap rate exceeds 5%
                {
                    uint64_t total_frames =
                        channel.stats.accepted_frames + channel.stats.dropped_frames;
                    float drop_rate =
                        (total_frames > 0)
                            ? static_cast<float>(channel.stats.dropped_frames) / total_frames
                            : 0.0f;
                    uint64_t gap_total =
                        channel.stats.inter_frame_gaps + channel.stats.accepted_frames;
                    float gap_rate =
                        (gap_total > 0)
                            ? static_cast<float>(channel.stats.inter_frame_gaps) / gap_total
                            : 0.0f;

                    if (drop_rate > kUncalDropRate || gap_rate > kUncalDropRate) {
                        ImPlot::PushPlotClipRect();
                        ImVec2 ppos = ImPlot::GetPlotPos();
                        ImVec2 psz = ImPlot::GetPlotSize();
                        ImGui::GetWindowDrawList()->AddText(ImVec2(ppos.x + psz.x - 70, ppos.y + 5),
                                                            IM_COL32(255, 60, 60, 255), "UNCAL");
                        ImPlot::PopPlotClipRect();
                    }
                }

                // Channel info overlay (top-right, below UNCAL if shown)
                {
                    ImPlot::PushPlotClipRect();
                    ImVec2 ppos = ImPlot::GetPlotPos();
                    ImVec2 psz = ImPlot::GetPlotSize();
                    char rate_buf[32], count_buf[32], info_txt[128];
                    snprintf(rate_buf, sizeof(rate_buf), "%s",
                             si_format(channel.sample_rate_hz, "Hz"));
                    snprintf(count_buf, sizeof(count_buf), "%s",
                             si_format(static_cast<double>(channel.packet_count), ""));
                    snprintf(info_txt, sizeof(info_txt), "%s  |  %s slots", rate_buf, count_buf);
                    ImGui::GetWindowDrawList()->AddText(
                        ImVec2(ppos.x + psz.x - ImGui::CalcTextSize(info_txt).x - 5, ppos.y + 20),
                        IM_COL32(180, 180, 180, 200), info_txt);
                    ImPlot::PopPlotClipRect();
                }

                // X-range readback on last subplot
                if (i == n - 1) {
                    ImPlotRect limits = ImPlot::GetPlotLimits();
                    state.visible_x_min = limits.X.Min;
                    state.visible_x_max = limits.X.Max;
                    state.has_valid_x_range = true;
                }

                ImPlot::EndPlot();
            }
        }
        ImPlot::EndSubplots();
    }
}

// ── SHM helper ───────────────────────────────────────────────────────────────

/// Add an SHM receiver by name. Normalizes, dedup-checks, starts, and pushes.
/// Writes status to state.channel_mgr_status. Returns true on success.
static bool add_shm_receiver(AppState &state, const char *name) {
    std::string bare = normalize_shm_name(name);
    if (bare.empty()) {
        snprintf(state.channel_mgr_status, sizeof(state.channel_mgr_status),
                 "Error: empty SHM name");
        return false;
    }

    // Duplicate name check (normalized)
    for (const auto &existing : state.shm_receivers) {
        std::string existing_bare = normalize_shm_name(existing->name().c_str());
        if (existing_bare == bare) {
            snprintf(state.channel_mgr_status, sizeof(state.channel_mgr_status),
                     "Error: SHM '%s' already attached", bare.c_str());
            return false;
        }
    }

    // Compute chan_id with collision detection
    uint16_t chan_id = pipscope::shm_chan_id(bare.c_str());
    for (uint16_t salt = 0; salt < 16; ++salt) {
        bool collision = false;
        for (const auto &existing : state.shm_receivers) {
            if (existing->chan_id() == chan_id) {
                collision = true;
                break;
            }
        }
        if (!collision)
            break;
        chan_id = pipscope::shm_chan_id(bare.c_str(), salt + 1);
    }

    auto shm_recv =
        std::make_unique<pipscope::ShmReceiver>(bare.c_str(), chan_id, state.buffer_capacity);
    if (!shm_recv->start()) {
        snprintf(state.channel_mgr_status, sizeof(state.channel_mgr_status),
                 "Error: failed to attach SHM '%s'", bare.c_str());
        return false;
    }

    snprintf(state.channel_mgr_status, sizeof(state.channel_mgr_status), "Added '%s' (chan_id=%u)",
             bare.c_str(), chan_id);
    printf("pipscope: attached SHM '%s' (chan_id=%u)\n", bare.c_str(), chan_id);
    state.shm_receivers.push_back(std::move(shm_recv));
    return true;
}

// ── Channel manager ──────────────────────────────────────────────────────────

static void do_disconnect(AppState &state, pipscope::PpktReceiver &receiver) {
    receiver.stop();
    receiver.clear_channels();
    state.raw_snapshots.clear();
    state.snapshots.clear();
    state.conn_status = ConnStatus::Disconnected;
    snprintf(state.status_msg, sizeof(state.status_msg), "Disconnected");
}

static void render_channel_manager(AppState &state, pipscope::PpktReceiver &receiver) {
    if (!state.show_channel_manager)
        return;

    ImGui::SetNextWindowSize(ImVec2(480, 400), ImGuiCond_FirstUseEver);
    if (!ImGui::Begin("Channel Manager", &state.show_channel_manager)) {
        ImGui::End();
        return;
    }

    // ── Socket section ───────────────────────────────────────────────────
    ImGui::SeparatorText("Socket");
    {
        ImVec4 status_color;
        switch (state.conn_status) {
        case ConnStatus::Connected:
            status_color = ImVec4(0.2f, 0.9f, 0.2f, 1.0f);
            break;
        case ConnStatus::Error:
            status_color = ImVec4(1.0f, 0.3f, 0.3f, 1.0f);
            break;
        default:
            status_color = ImVec4(0.6f, 0.6f, 0.6f, 1.0f);
            break;
        }
        ImGui::TextColored(status_color, "%s", state.status_msg);

        if (state.conn_status == ConnStatus::Connected) {
            ImGui::SameLine();
            if (ImGui::Button("Disconnect")) {
                do_disconnect(state, receiver);
            }
        }

        // List PPKT channels (label.empty() entries in raw_snapshots)
        ImGui::PushID("socket");
        int socket_idx = 0;
        for (const auto &snap : state.raw_snapshots) {
            if (!snap.label.empty())
                continue; // skip SHM channels
            ImGui::PushID(socket_idx++);
            ChannelKey key{SourceKind::Socket, snap.chan_id};
            bool visible = state.hidden_channels.count(key) == 0;
            char ch_label[64];
            snprintf(ch_label, sizeof(ch_label), "Ch %u  (%s)", snap.chan_id,
                     si_format(snap.sample_rate_hz, "Hz"));
            if (ImGui::Checkbox(ch_label, &visible)) {
                if (visible)
                    state.hidden_channels.erase(key);
                else
                    state.hidden_channels.insert(key);
            }
            ImGui::PopID();
        }
        ImGui::PopID();
    }

    // ── SHM section ──────────────────────────────────────────────────────
    ImGui::SeparatorText("SHM");
    {
        ImGui::PushID("shm");
        for (int i = static_cast<int>(state.shm_receivers.size()) - 1; i >= 0; --i) {
            auto &shm = state.shm_receivers[static_cast<size_t>(i)];
            ImGui::PushID(i);

            ChannelKey key{SourceKind::Shm, shm->chan_id()};
            bool visible = state.hidden_channels.count(key) == 0;

            // Find sample rate from raw_snapshots
            double rate = 0.0;
            for (const auto &snap : state.raw_snapshots) {
                if (snap.chan_id == shm->chan_id() && !snap.label.empty()) {
                    rate = snap.sample_rate_hz;
                    break;
                }
            }

            char ch_label[128];
            snprintf(ch_label, sizeof(ch_label), "shm:%s  (%s)", shm->name().c_str(),
                     si_format(rate, "Hz"));
            if (ImGui::Checkbox(ch_label, &visible)) {
                if (visible)
                    state.hidden_channels.erase(key);
                else
                    state.hidden_channels.insert(key);
            }

            ImGui::SameLine();
            if (ImGui::Button("Remove")) {
                uint16_t remove_id = shm->chan_id();
                state.hidden_channels.erase(key);
                shm->stop();
                state.shm_receivers.erase(state.shm_receivers.begin() + i);

                // Purge waveform data for the removed channel
                auto pred = [remove_id](const pipscope::ChannelSnapshot &s) {
                    return s.chan_id == remove_id && !s.label.empty();
                };
                state.raw_snapshots.erase(
                    std::remove_if(state.raw_snapshots.begin(), state.raw_snapshots.end(), pred),
                    state.raw_snapshots.end());
                state.snapshots.erase(
                    std::remove_if(state.snapshots.begin(), state.snapshots.end(), pred),
                    state.snapshots.end());
            }

            ImGui::PopID();
        }
        ImGui::PopID();
    }

    // ── Add Channel section ──────────────────────────────────────────────
    ImGui::SeparatorText("Add Channel");
    {
        const char *type_items[] = {"Socket", "SHM"};
        ImGui::SetNextItemWidth(100);
        ImGui::Combo("Type", &state.add_source_type, type_items, 2);
        ImGui::SameLine();

        const char *hint = state.add_source_type == 0 ? "host:port" : "ring name";
        ImGui::SetNextItemWidth(200);
        bool enter =
            ImGui::InputText("##add_input", state.add_input_buf, sizeof(state.add_input_buf),
                             ImGuiInputTextFlags_EnterReturnsTrue);
        if (strlen(state.add_input_buf) == 0 && !ImGui::IsItemActive()) {
            // Draw hint text when empty and not focused
            ImVec2 pos = ImGui::GetItemRectMin();
            ImGui::GetWindowDrawList()->AddText(ImVec2(pos.x + 4, pos.y + 3),
                                                IM_COL32(128, 128, 128, 128), hint);
        }
        ImGui::SameLine();

        const char *btn_label = state.add_source_type == 0 ? "Connect" : "Add";
        if (ImGui::Button(btn_label) || enter) {
            if (state.add_source_type == 0) {
                // Socket: copy address and connect
                strncpy(state.address_buf, state.add_input_buf, sizeof(state.address_buf) - 1);
                state.address_buf[sizeof(state.address_buf) - 1] = '\0';
                do_connect(state, receiver);
                snprintf(state.channel_mgr_status, sizeof(state.channel_mgr_status), "%s",
                         state.status_msg);
            } else {
                // SHM: add receiver
                add_shm_receiver(state, state.add_input_buf);
            }
        }

        // Status line
        if (state.channel_mgr_status[0] != '\0') {
            bool is_error = (strncmp(state.channel_mgr_status, "Error", 5) == 0 ||
                             strncmp(state.channel_mgr_status, "Failed", 6) == 0);
            ImVec4 color =
                is_error ? ImVec4(1.0f, 0.3f, 0.3f, 1.0f) : ImVec4(0.2f, 0.9f, 0.2f, 1.0f);
            ImGui::TextColored(color, "%s", state.channel_mgr_status);
        }
    }

    ImGui::End();
}

static void render_ui(AppState &state, pipscope::PpktReceiver &receiver) {
    ImGui::SetNextWindowPos(ImVec2(0, 0));
    ImGui::SetNextWindowSize(ImGui::GetIO().DisplaySize);
    ImGui::Begin("pipscope", nullptr,
                 ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                     ImGuiWindowFlags_NoCollapse | ImGuiWindowFlags_NoBringToFrontOnFocus);

    render_toolbar(state, receiver);
    ImGui::Separator();

    update_snapshots(state, receiver);

    // Filter hidden channels from display snapshots
    if (!state.hidden_channels.empty()) {
        state.snapshots.erase(
            std::remove_if(state.snapshots.begin(), state.snapshots.end(),
                           [&](const pipscope::ChannelSnapshot &ch) {
                               SourceKind kind =
                                   ch.label.empty() ? SourceKind::Socket : SourceKind::Shm;
                               return state.hidden_channels.count(ChannelKey{kind, ch.chan_id}) > 0;
                           }),
            state.snapshots.end());
    }

    // Auto-select trigger source when current source is invalid
    {
        bool source_exists = false;
        for (const auto &snap : state.raw_snapshots) {
            if (snap.chan_id == state.trigger.source_chan_id) {
                source_exists = true;
                break;
            }
        }
        if (!source_exists && !state.snapshots.empty())
            state.trigger.source_chan_id = state.snapshots[0].chan_id;
    }

    if (state.snapshots.empty()) {
        if (!state.raw_snapshots.empty()) {
            ImGui::TextDisabled("All channels hidden");
        } else {
            bool has_any_source =
                state.conn_status == ConnStatus::Connected || !state.shm_receivers.empty();
            if (has_any_source) {
                ImGui::TextDisabled("Waiting for data ...");
            } else {
                ImGui::TextDisabled("Use the Channels button to add data sources.");
            }
        }
    }

    render_channels(state);

    // Auto-compute display_samples from visible x-range (after render_channels
    // readback). 10% hysteresis prevents feedback jitter.
    if (state.has_valid_x_range) {
        double visible_range = state.visible_x_max - state.visible_x_min;
        if (visible_range > 0) {
            double max_rate = 0;
            for (const auto &ch : state.snapshots) {
                if (ch.sample_rate_hz > max_rate)
                    max_rate = ch.sample_rate_hz;
            }
            if (max_rate > 0) {
                int computed = static_cast<int>(
                    std::min(max_rate * visible_range, static_cast<double>(state.buffer_capacity)));
                if (computed < 64)
                    computed = 64;
                if (std::abs(computed - state.display_samples) > state.display_samples / 10) {
                    state.display_samples = computed;
                }
            }
        }
    }

    ImGui::End();

    // Channel Manager (separate moveable ImGui window)
    render_channel_manager(state, receiver);
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main(int argc, char **argv) {
    pipscope::CliArgs args = pipscope::parse_args(argc, argv);
    if (args.error) {
        pipscope::print_usage(argv[0]);
        return 1;
    }

    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    GLFWwindow *window = nullptr;
    if (!init_window(window, args.vsync)) {
        return 1;
    }

    init_imgui(window);

    pipscope::PpktReceiver receiver;
    AppState state;
    state.buffer_capacity = receiver.buffer_capacity();
    state.snapshot_hz = args.snapshot_hz;

    // WSL/WSLg can intermittently stop honoring swap interval while hovering
    // or interacting with windows. Keep a software frame cap as a stability
    // fallback to avoid extreme frame spikes that look like flicker.
    int frame_cap_fps = 0;
    if (const char *fps_env = getenv("PIPSCOPE_MAX_FPS")) {
        frame_cap_fps = atoi(fps_env);
        if (frame_cap_fps < 0)
            frame_cap_fps = 0;
    } else if (is_wsl2()) {
        frame_cap_fps = 60;
        fprintf(stderr,
                "pipscope: WSL detected; enabling frame cap %d FPS "
                "(set PIPSCOPE_MAX_FPS=0 to disable)\n",
                frame_cap_fps);
    }

    if (args.has_address) {
        strncpy(state.address_buf, args.address, sizeof(state.address_buf) - 1);
        do_connect(state, receiver);
        if (state.conn_status == ConnStatus::Error) {
            fprintf(stderr, "Error: %s\n", state.status_msg);
            shutdown_imgui();
            shutdown_window(window);
            return 1;
        }
        printf("pipscope: %s\n", state.status_msg);
    }

    // Start SHM receivers (fail-soft: warn and skip on failure)
    for (const auto &shm_name : args.shm_names) {
        if (!add_shm_receiver(state, shm_name.c_str())) {
            fprintf(stderr, "pipscope: warning: %s\n", state.channel_mgr_status);
        }
    }
    state.channel_mgr_status[0] = '\0'; // clear startup messages

    while (!glfwWindowShouldClose(window) && !g_shutdown) {
        auto frame_start = std::chrono::steady_clock::now();
        glfwPollEvents();
        if (glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS)
            break;
        begin_frame();
        render_ui(state, receiver);
        end_frame(window);
        if (frame_cap_fps > 0) {
            auto frame_budget =
                std::chrono::duration<double>(1.0 / static_cast<double>(frame_cap_fps));
            auto frame_deadline =
                frame_start +
                std::chrono::duration_cast<std::chrono::steady_clock::duration>(frame_budget);
            std::this_thread::sleep_until(frame_deadline);
        }
    }

    for (auto &shm_recv : state.shm_receivers) {
        shm_recv->stop();
    }
    state.shm_receivers.clear();
    receiver.stop();
    shutdown_imgui();
    shutdown_window(window);
    printf("pipscope: shutdown complete\n");
    return 0;
}
