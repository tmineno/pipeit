//
// pipscope — Oscilloscope GUI for Pipit PPKT streams
//
// Receives PPKT packets over UDP and displays real-time waveforms using
// ImGui + ImPlot.  See doc/spec/ppkt-protocol-spec-v0.2.x.md for protocol details.
//
// Usage: pipscope --port <port>
//        pipscope -p <port>
//

#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>

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

static int parse_port(int argc, char **argv) {
    for (int i = 1; i < argc - 1; i++) {
        if (strcmp(argv[i], "--port") == 0 || strcmp(argv[i], "-p") == 0) {
            int port = atoi(argv[i + 1]);
            if (port > 0 && port <= 65535)
                return port;
            fprintf(stderr, "Error: invalid port '%s'\n", argv[i + 1]);
            return -1;
        }
    }
    return -1; // not found
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main(int argc, char **argv) {
    int port = parse_port(argc, argv);
    if (port < 0) {
        print_usage(argv[0]);
        return 1;
    }

    // Signal handlers for clean shutdown
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    // ── GLFW init ────────────────────────────────────────────────────────

    if (!glfwInit()) {
        fprintf(stderr, "Error: failed to initialize GLFW.\n"
                        "Is a display server running (WSLg/X11/Wayland)?\n");
        return 1;
    }

    glfwWindowHint(GLFW_CONTEXT_VERSION_MAJOR, 3);
    glfwWindowHint(GLFW_CONTEXT_VERSION_MINOR, 3);
    glfwWindowHint(GLFW_OPENGL_PROFILE, GLFW_OPENGL_CORE_PROFILE);

    GLFWwindow *window = glfwCreateWindow(1280, 720, "pipscope", nullptr, nullptr);
    if (!window) {
        fprintf(stderr, "Error: failed to create GLFW window.\n");
        glfwTerminate();
        return 1;
    }
    glfwMakeContextCurrent(window);
    glfwSwapInterval(1); // vsync

    // ── ImGui + ImPlot init ──────────────────────────────────────────────

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImPlot::CreateContext();

    ImGui::StyleColorsDark();
    ImGui_ImplGlfw_InitForOpenGL(window, true);
    ImGui_ImplOpenGL3_Init("#version 330");

    // ── Start PPKT receiver ──────────────────────────────────────────────

    pipscope::PpktReceiver receiver;
    if (!receiver.start(static_cast<uint16_t>(port))) {
        fprintf(stderr, "Error: failed to bind UDP port %d\n", port);
        ImGui_ImplOpenGL3_Shutdown();
        ImGui_ImplGlfw_Shutdown();
        ImPlot::DestroyContext();
        ImGui::DestroyContext();
        glfwDestroyWindow(window);
        glfwTerminate();
        return 1;
    }

    printf("pipscope: listening on UDP port %d\n", port);

    // ── Application state ────────────────────────────────────────────────

    bool paused = false;
    bool auto_y = true;
    int display_samples = 8192;
    std::vector<pipscope::ChannelSnapshot> snapshots;

    // ── Render loop ──────────────────────────────────────────────────────

    while (!glfwWindowShouldClose(window) && !g_shutdown) {
        glfwPollEvents();

        ImGui_ImplOpenGL3_NewFrame();
        ImGui_ImplGlfw_NewFrame();
        ImGui::NewFrame();

        // Full-window ImGui panel
        ImGui::SetNextWindowPos(ImVec2(0, 0));
        ImGui::SetNextWindowSize(ImGui::GetIO().DisplaySize);
        ImGui::Begin("pipscope", nullptr,
                     ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize |
                         ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoCollapse |
                         ImGuiWindowFlags_NoBringToFrontOnFocus);

        // Toolbar
        if (ImGui::Button(paused ? "Resume" : "Pause"))
            paused = !paused;
        ImGui::SameLine();
        ImGui::Checkbox("Auto-Y", &auto_y);
        ImGui::SameLine();
        ImGui::SetNextItemWidth(200);
        ImGui::SliderInt("Samples", &display_samples, 64, 65536, "%d",
                         ImGuiSliderFlags_Logarithmic);
        ImGui::SameLine();
        ImGui::Text("| Port: %d", port);

        ImGui::Separator();

        // Get snapshot (unless paused)
        if (!paused) {
            snapshots = receiver.snapshot(static_cast<size_t>(display_samples));
        }

        if (snapshots.empty()) {
            ImGui::TextDisabled("Waiting for PPKT data on UDP port %d ...", port);
        }

        // Render one plot per channel
        float plot_height =
            snapshots.size() <= 1 ? -1.0f : ImGui::GetContentRegionAvail().y / snapshots.size();
        if (plot_height > 0 && plot_height < 150)
            plot_height = 150;

        for (auto &ch : snapshots) {
            char label[128];
            snprintf(label, sizeof(label), "Channel %u  |  %.0f Hz  |  %lu pkts", ch.chan_id,
                     ch.sample_rate_hz, static_cast<unsigned long>(ch.packet_count));
            ImGui::Text("%s", label);

            char plot_id[32];
            snprintf(plot_id, sizeof(plot_id), "##ch%u", ch.chan_id);

            ImPlotFlags flags = ImPlotFlags_None;
            if (ImPlot::BeginPlot(plot_id, ImVec2(-1, plot_height), flags)) {
                ImPlot::SetupAxes("Time (s)", "Amplitude");
                if (auto_y)
                    ImPlot::SetupAxis(ImAxis_Y1, nullptr, ImPlotAxisFlags_AutoFit);

                if (!ch.samples.empty()) {
                    double dt = (ch.sample_rate_hz > 0) ? 1.0 / ch.sample_rate_hz : 1.0;
                    int n = static_cast<int>(ch.samples.size());
                    ImPlot::PlotLine("signal", ch.samples.data(), n, dt, 0.0);
                }
                ImPlot::EndPlot();
            }
        }

        ImGui::End();

        // Render
        ImGui::Render();
        int display_w, display_h;
        glfwGetFramebufferSize(window, &display_w, &display_h);
        glViewport(0, 0, display_w, display_h);
        glClearColor(0.06f, 0.06f, 0.06f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
        glfwSwapBuffers(window);
    }

    // ── Cleanup ──────────────────────────────────────────────────────────

    receiver.stop();

    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplGlfw_Shutdown();
    ImPlot::DestroyContext();
    ImGui::DestroyContext();
    glfwDestroyWindow(window);
    glfwTerminate();

    printf("pipscope: shutdown complete\n");
    return 0;
}
