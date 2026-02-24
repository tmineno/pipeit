//
// pipscope — Oscilloscope GUI for Pipit PPKT streams
//
// Receives PPKT packets over UDP and displays real-time waveforms using
// ImGui + ImPlot.  See doc/spec/ppkt-protocol-spec-v0.3.0.md for protocol details.
//
// Usage: pipscope [--port <port>] [--address <addr>]
//        pipscope [-p <port>] [-a <addr>]
//

#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
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
#include "trigger.h"

// ── Signal handling ──────────────────────────────────────────────────────────

static volatile sig_atomic_t g_shutdown = 0;

static void signal_handler(int) { g_shutdown = 1; }

// ── CLI parsing (implemented in cli.h) ───────────────────────────────────────

static bool init_window(GLFWwindow *&window, bool vsync = false) {
    if (!glfwInit()) {
        fprintf(stderr, "Error: failed to initialize GLFW.\n"
                        "Is a display server running (WSLg/X11/Wayland)?\n");
        return false;
    }

    glfwWindowHint(GLFW_CONTEXT_VERSION_MAJOR, 3);
    glfwWindowHint(GLFW_CONTEXT_VERSION_MINOR, 3);
    glfwWindowHint(GLFW_OPENGL_PROFILE, GLFW_OPENGL_CORE_PROFILE);

    window = glfwCreateWindow(1280, 720, "pipscope", nullptr, nullptr);
    if (!window) {
        fprintf(stderr, "Error: failed to create GLFW window.\n");
        glfwTerminate();
        return false;
    }

    glfwMakeContextCurrent(window);
    glfwSwapInterval(vsync ? 1 : 0);
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
    if (ImGui::Button(state.paused ? "Resume" : "Pause")) {
        state.paused = !state.paused;
    }
    ImGui::SameLine();
    ImGui::Checkbox("Auto-Y", &state.auto_y);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(200);
    ImGui::SliderInt("Samples", &state.display_samples, 64, 65536, "%d",
                     ImGuiSliderFlags_Logarithmic);

    ImGui::SameLine();
    ImGui::Text("|");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(200);
    bool enter_pressed = ImGui::InputText("##address", state.address_buf, sizeof(state.address_buf),
                                          ImGuiInputTextFlags_EnterReturnsTrue);
    ImGui::SameLine();
    if (ImGui::Button("Connect") || enter_pressed) {
        do_connect(state, receiver);
    }

    ImGui::SameLine();
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
    ImGui::SameLine();
    ImGui::Text("| Refresh: %.0f Hz", state.refresh_rate_hz);
    ImGui::SameLine();
    ImGui::Text("| recv: %.1fk pps  %.1f MB/s | snap: %.2f ms", state.recv_pps / 1000.0f,
                state.recv_mbps, state.snapshot_ms);

    // ── Trigger controls (row 2) ─────────────────────────────────────────
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

    // Source channel combo — populated from current snapshot chan_ids
    {
        char combo_label[32];
        snprintf(combo_label, sizeof(combo_label), "Ch %u", state.trigger.source_chan_id);
        ImGui::SetNextItemWidth(80);
        if (ImGui::BeginCombo("Source", combo_label)) {
            for (const auto &snap : state.snapshots) {
                char item_label[32];
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

static float plot_height_for_channels(size_t channel_count) {
    float plot_height =
        channel_count <= 1 ? -1.0f : ImGui::GetContentRegionAvail().y / channel_count;
    if (plot_height > 0.0f && plot_height < 150.0f) {
        return 150.0f;
    }
    return plot_height;
}

static void render_channel_plot(const pipscope::ChannelSnapshot &channel, bool auto_y,
                                float plot_height, pipscope::TriggerConfig &trigger) {
    char label[256];
    snprintf(label, sizeof(label),
             "Channel %u  |  %.0f Hz  |  %lu pkts  |  frames: %lu ok / %lu dropped",
             channel.chan_id, channel.sample_rate_hz,
             static_cast<unsigned long>(channel.packet_count),
             static_cast<unsigned long>(channel.stats.accepted_frames),
             static_cast<unsigned long>(channel.stats.dropped_frames));
    ImGui::Text("%s", label);

    if (channel.stats.dropped_frames > 0) {
        ImGui::SameLine();
        ImGui::TextColored(ImVec4(1.0f, 0.6f, 0.2f, 1.0f), "(seq:%lu iter:%lu bnd:%lu meta:%lu)",
                           static_cast<unsigned long>(channel.stats.drop_seq_gap),
                           static_cast<unsigned long>(channel.stats.drop_iter_gap),
                           static_cast<unsigned long>(channel.stats.drop_boundary),
                           static_cast<unsigned long>(channel.stats.drop_meta_mismatch));
    }
    if (channel.stats.inter_frame_gaps > 0) {
        ImGui::SameLine();
        ImGui::TextColored(ImVec4(1.0f, 1.0f, 0.2f, 1.0f), "[pkt loss: %lu]",
                           static_cast<unsigned long>(channel.stats.inter_frame_gaps));
    }

    char plot_id[32];
    snprintf(plot_id, sizeof(plot_id), "##ch%u", channel.chan_id);

    if (ImPlot::BeginPlot(plot_id, ImVec2(-1, plot_height), ImPlotFlags_None)) {
        ImPlot::SetupAxes("Time (s)", "Amplitude");
        if (auto_y) {
            ImPlot::SetupAxis(ImAxis_Y1, nullptr, ImPlotAxisFlags_AutoFit);
        }
        // Constrain x-axis to the data range. Because all update paths guarantee exactly
        // display_samples samples (via take_tail + guard), data_duration is stable and won't
        // cause axis jitter. The constraint prevents zoom-out beyond data but allows zoom-in.
        if (channel.sample_rate_hz > 0 && !channel.samples.empty()) {
            double data_duration =
                static_cast<double>(channel.samples.size()) / channel.sample_rate_hz;
            ImPlot::SetupAxisLimitsConstraints(ImAxis_X1, 0.0, data_duration);
        }

        if (!channel.samples.empty()) {
            double dt = channel.sample_rate_hz > 0 ? 1.0 / channel.sample_rate_hz : 1.0;
            size_t n = channel.samples.size();
            static constexpr size_t kMaxPlotPoints = 4000;

            if (n > kMaxPlotPoints) {
                int factor = static_cast<int>(n / (kMaxPlotPoints / 2));
                // thread_local buffers avoid per-frame allocation
                thread_local std::vector<float> dec_x, dec_y;
                size_t max_out = 2 * ((n + factor - 1) / factor);
                dec_x.resize(max_out);
                dec_y.resize(max_out);
                size_t dn = pipscope::decimate_minmax(channel.samples.data(), n, factor,
                                                      dec_x.data(), dec_y.data(), dt);
                if (dn > 0) {
                    ImPlot::PlotLine("signal", dec_x.data(), dec_y.data(), static_cast<int>(dn));
                }
            } else {
                ImPlot::PlotLine("signal", channel.samples.data(), static_cast<int>(n), dt, 0.0);
            }
        }

        // Trigger level line (draggable, yellow) on the source channel
        if (trigger.enabled && channel.chan_id == trigger.source_chan_id) {
            double trig_level = static_cast<double>(trigger.level);
            ImPlot::DragLineY(0, &trig_level, ImVec4(1, 1, 0, 1), 1, ImPlotDragToolFlags_NoFit);
            trigger.level = static_cast<float>(trig_level);
        }

        // "Waiting for trigger..." overlay
        if (trigger.waiting) {
            ImPlot::PushPlotClipRect();
            ImVec2 pos = ImPlot::GetPlotPos();
            ImVec2 sz = ImPlot::GetPlotSize();
            ImGui::GetWindowDrawList()->AddText(
                ImVec2(pos.x + sz.x * 0.5f - 70, pos.y + sz.y * 0.5f - 8),
                IM_COL32(200, 200, 200, 180), "Waiting for trigger...");
            ImPlot::PopPlotClipRect();
        }

        // UNCAL overlay — drop rate or inter-frame gap rate exceeds 5%
        {
            uint64_t total_frames = channel.stats.accepted_frames + channel.stats.dropped_frames;
            float drop_rate = (total_frames > 0)
                                  ? static_cast<float>(channel.stats.dropped_frames) / total_frames
                                  : 0.0f;

            uint64_t gap_total = channel.stats.inter_frame_gaps + channel.stats.accepted_frames;
            float gap_rate = (gap_total > 0)
                                 ? static_cast<float>(channel.stats.inter_frame_gaps) / gap_total
                                 : 0.0f;

            if (drop_rate > kUncalDropRate || gap_rate > kUncalDropRate) {
                ImPlot::PushPlotClipRect();
                ImVec2 pos = ImPlot::GetPlotPos();
                ImVec2 sz = ImPlot::GetPlotSize();
                ImGui::GetWindowDrawList()->AddText(ImVec2(pos.x + sz.x - 70, pos.y + 5),
                                                    IM_COL32(255, 60, 60, 255), "UNCAL");
                ImPlot::PopPlotClipRect();
            }
        }

        ImPlot::EndPlot();
    }
}

static void render_channels(AppState &state) {
    float plot_height = plot_height_for_channels(state.snapshots.size());
    for (const auto &channel : state.snapshots) {
        render_channel_plot(channel, state.auto_y, plot_height, state.trigger);
    }
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
    if (state.snapshots.empty()) {
        if (state.conn_status == ConnStatus::Connected) {
            ImGui::TextDisabled("Waiting for PPKT data on %s ...", state.address_buf);
        } else if (state.conn_status == ConnStatus::Disconnected) {
            ImGui::TextDisabled("Enter an address and click Connect to start.");
        }
    }

    render_channels(state);
    ImGui::End();
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

    while (!glfwWindowShouldClose(window) && !g_shutdown) {
        glfwPollEvents();
        if (glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS)
            break;
        begin_frame();
        render_ui(state, receiver);
        end_frame(window);
    }

    receiver.stop();
    shutdown_imgui();
    shutdown_window(window);
    printf("pipscope: shutdown complete\n");
    return 0;
}
