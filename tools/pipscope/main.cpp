//
// pipscope — Oscilloscope GUI for Pipit PPKT streams
//
// Receives PPKT packets over UDP and displays real-time waveforms using
// ImGui + ImPlot.  See doc/spec/ppkt-protocol-spec-v0.3.0.md for protocol details.
//
// Usage: pipscope [--port <port>] [--address <addr>]
//        pipscope [-p <port>] [-a <addr>]
//

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

#include "ppkt_receiver.h"
#include "trigger.h"

// ── Signal handling ──────────────────────────────────────────────────────────

static volatile sig_atomic_t g_shutdown = 0;

static void signal_handler(int) { g_shutdown = 1; }

// ── CLI parsing ──────────────────────────────────────────────────────────────

static void print_usage(const char *argv0) {
    fprintf(stderr, "Usage: %s [--port <port>] [--address <addr>]\n", argv0);
    fprintf(stderr, "       %s [-p <port>] [-a <addr>]\n", argv0);
    fprintf(stderr, "\n");
    fprintf(stderr, "  -p, --port <port>       Listen on 0.0.0.0:<port> (UDP)\n");
    fprintf(stderr, "  -a, --address <addr>    Listen on <addr> (e.g. localhost:9100)\n");
    fprintf(stderr, "  -h, --help              Show this help message\n");
    fprintf(stderr, "\nIf no address is given, starts with GUI address input.\n");
}

struct CliArgs {
    char address[128] = {};
    bool has_address = false;
    bool error = false;
};

static CliArgs parse_args(int argc, char **argv) {
    CliArgs args{};
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            exit(0);
        }
        if ((strcmp(argv[i], "--port") == 0 || strcmp(argv[i], "-p") == 0) && i + 1 < argc) {
            int port = atoi(argv[i + 1]);
            if (port > 0 && port <= 65535) {
                snprintf(args.address, sizeof(args.address), "0.0.0.0:%d", port);
                args.has_address = true;
            } else {
                fprintf(stderr, "Error: invalid port '%s'\n", argv[i + 1]);
                args.error = true;
            }
            i++;
            continue;
        }
        if ((strcmp(argv[i], "--address") == 0 || strcmp(argv[i], "-a") == 0) && i + 1 < argc) {
            size_t len = strlen(argv[i + 1]);
            if (len > 0 && len < sizeof(args.address)) {
                strncpy(args.address, argv[i + 1], sizeof(args.address) - 1);
                args.has_address = true;
            } else {
                fprintf(stderr, "Error: invalid address '%s'\n", argv[i + 1]);
                args.error = true;
            }
            i++;
            continue;
        }
    }
    return args;
}

static bool init_window(GLFWwindow *&window) {
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
    glfwSwapInterval(1);
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
            dst[c].samples.assign(src[c].samples.begin() + start, src[c].samples.begin() + end);
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
            dst[c].samples.assign(src[c].samples.end() - n, src[c].samples.end());
        } else {
            dst[c].samples.clear();
        }
    }
}

static void update_snapshots(AppState &state, pipscope::PpktReceiver &receiver) {
    if (state.paused)
        return;

    int ds = state.display_samples;
    size_t request_size =
        state.trigger.enabled ? static_cast<size_t>(ds) * 2 : static_cast<size_t>(ds);
    auto raw = receiver.snapshot(request_size);

    // X-axis guard: if we already have valid display data AND any channel in raw
    // has fewer samples than display_samples, hold the previous display.
    if (!state.snapshots.empty()) {
        for (const auto &ch : raw) {
            if (static_cast<int>(ch.samples.size()) < ds)
                return; // hold previous display
        }
    }

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
        } else if (state.trigger.mode == pipscope::TriggerConfig::Auto) {
            state.trigger.waiting = false;
            take_tail(state.snapshots, raw, ds);
        } else {
            // Normal mode: no trigger found — keep previous display
            state.trigger.waiting = true;
        }
    } else {
        state.trigger.waiting = false;
        state.snapshots = std::move(raw);
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

        if (!channel.samples.empty()) {
            double dt = channel.sample_rate_hz > 0 ? 1.0 / channel.sample_rate_hz : 1.0;
            int n = static_cast<int>(channel.samples.size());
            ImPlot::PlotLine("signal", channel.samples.data(), n, dt, 0.0);
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
    CliArgs args = parse_args(argc, argv);
    if (args.error) {
        print_usage(argv[0]);
        return 1;
    }

    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    GLFWwindow *window = nullptr;
    if (!init_window(window)) {
        return 1;
    }

    init_imgui(window);

    pipscope::PpktReceiver receiver;
    AppState state;

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
