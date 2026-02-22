//
// pipscope — Oscilloscope GUI for Pipit PPKT streams
//
// Receives PPKT packets over UDP and displays real-time waveforms using
// ImGui + ImPlot.  See doc/spec/ppkt-protocol-spec-v0.3.0.md for protocol details.
//
// Usage: pipscope --port <port>
//        pipscope -p <port>
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

// ── Signal handling ──────────────────────────────────────────────────────────

static volatile sig_atomic_t g_shutdown = 0;

static void signal_handler(int) { g_shutdown = 1; }

// ── CLI parsing ──────────────────────────────────────────────────────────────

static void print_usage(const char *argv0) {
    fprintf(stderr, "Usage: %s --port <port>\n", argv0);
    fprintf(stderr, "       %s -p <port>\n", argv0);
}

static bool parse_port(int argc, char **argv, int &port_out) {
    for (int i = 1; i + 1 < argc; i++) {
        if (strcmp(argv[i], "--port") == 0 || strcmp(argv[i], "-p") == 0) {
            int port = atoi(argv[i + 1]);
            if (port > 0 && port <= 65535) {
                port_out = port;
                return true;
            }
            fprintf(stderr, "Error: invalid port '%s'\n", argv[i + 1]);
            return false;
        }
    }
    return false;
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

struct AppState {
    bool paused = false;
    bool auto_y = true;
    int display_samples = 8192;
    std::vector<pipscope::ChannelSnapshot> snapshots;
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

static void render_toolbar(AppState &state, int port) {
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
    ImGui::Text("| Port: %d", port);
}

static void update_snapshots(AppState &state, pipscope::PpktReceiver &receiver) {
    if (!state.paused) {
        state.snapshots = receiver.snapshot(static_cast<size_t>(state.display_samples));
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
                                float plot_height) {
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
        ImPlot::EndPlot();
    }
}

static void render_channels(const AppState &state) {
    float plot_height = plot_height_for_channels(state.snapshots.size());
    for (const auto &channel : state.snapshots) {
        render_channel_plot(channel, state.auto_y, plot_height);
    }
}

static void render_ui(AppState &state, int port, pipscope::PpktReceiver &receiver) {
    ImGui::SetNextWindowPos(ImVec2(0, 0));
    ImGui::SetNextWindowSize(ImGui::GetIO().DisplaySize);
    ImGui::Begin("pipscope", nullptr,
                 ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                     ImGuiWindowFlags_NoCollapse | ImGuiWindowFlags_NoBringToFrontOnFocus);

    render_toolbar(state, port);
    ImGui::Separator();

    update_snapshots(state, receiver);
    if (state.snapshots.empty()) {
        ImGui::TextDisabled("Waiting for PPKT data on UDP port %d ...", port);
    }

    render_channels(state);
    ImGui::End();
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main(int argc, char **argv) {
    int port = 0;
    if (!parse_port(argc, argv, port)) {
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
    if (!receiver.start(static_cast<uint16_t>(port))) {
        fprintf(stderr, "Error: failed to bind UDP port %d\n", port);
        shutdown_imgui();
        shutdown_window(window);
        return 1;
    }
    printf("pipscope: listening on UDP port %d\n", port);

    AppState state;
    while (!glfwWindowShouldClose(window) && !g_shutdown) {
        glfwPollEvents();
        begin_frame();
        render_ui(state, port, receiver);
        end_frame(window);
    }

    receiver.stop();
    shutdown_imgui();
    shutdown_window(window);
    printf("pipscope: shutdown complete\n");
    return 0;
}
