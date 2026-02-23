#pragma once

// pipit_shell.h — Pipit runtime shell orchestration library
//
// Provides the generic runtime shell for compiled Pipit pipelines.
// Generated code supplies descriptor tables (params, tasks, buffers, probes)
// and calls shell_main() instead of emitting inline CLI parsing, thread
// management, and statistics output.
//
// See ADR-026 for design rationale.

#include <pipit.h>

#include <atomic>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstring>
#include <limits>
#include <span>
#include <string>
#include <thread>
#include <unordered_set>
#include <vector>

namespace pipit {

// ── Descriptor types ────────────────────────────────────────────────────────

struct ParamDesc {
    const char *name;
    bool (*apply)(const char *value); // parse CLI string + store; false on error
};

struct TaskDesc {
    const char *name;
    void (*entry)();  // task function pointer
    TaskStats *stats; // per-task stats accumulator
};

struct BufferStatsDesc {
    const char *name;
    size_t (*available)(); // returns available token count
    size_t elem_size;      // sizeof element type
};

struct ProbeDesc {
    const char *name;
    bool *enabled; // pointer to per-probe enable flag
};

struct RuntimeState {
    std::atomic<bool> *stop;
    std::atomic<int> *exit_code;
    std::atomic<bool> *start;
    bool *stats;
    FILE **probe_output; // always valid: points to generated _probe_output_file
};

struct ProgramDesc {
    RuntimeState state;
    std::span<const ParamDesc> params;
    std::span<const TaskDesc> tasks;
    std::span<const BufferStatsDesc> buffers;
    std::span<const ProbeDesc> probes;
    const char *overrun_policy;
    size_t mem_allocated;
    size_t mem_used;
};

// ── Shell entry point ───────────────────────────────────────────────────────

namespace detail {

inline bool parse_duration(const std::string &s, double *out) {
    if (s == "inf") {
        *out = std::numeric_limits<double>::infinity();
        return true;
    }
    std::size_t pos = 0;
    double base = 0.0;
    try {
        base = std::stod(s, &pos);
    } catch (...) {
        return false;
    }
    std::string unit = s.substr(pos);
    if (unit.empty() || unit == "s") {
        *out = base;
        return true;
    }
    if (unit == "m") {
        *out = base * 60.0;
        return true;
    }
    return false;
}

} // namespace detail

inline int shell_main(int argc, char *argv[], const ProgramDesc &desc) {
    double duration_seconds = std::numeric_limits<double>::infinity();
    int threads = 0;
    std::string probe_output_path = "/dev/stderr";
    std::vector<std::string> enabled_probes;

    // ── CLI argument parsing ────────────────────────────────────────────
    for (int i = 1; i < argc; ++i) {
        std::string opt(argv[i]);
        if (opt == "--param") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "startup error: --param requires name=value\n");
                return 2;
            }
            std::string arg(argv[++i]);
            auto eq = arg.find('=');
            if (eq == std::string::npos) {
                std::fprintf(stderr, "startup error: --param requires name=value\n");
                return 2;
            }
            auto name = arg.substr(0, eq);
            auto val = arg.substr(eq + 1);
            bool found = false;
            for (const auto &p : desc.params) {
                if (name == p.name) {
                    if (!p.apply(val.c_str())) {
                        std::fprintf(stderr, "startup error: invalid value '%s' for param '%s'\n",
                                     val.c_str(), name.c_str());
                        return 2;
                    }
                    found = true;
                    break;
                }
            }
            if (!found) {
                if (desc.params.empty()) {
                    std::fprintf(stderr,
                                 "startup error: --param is unsupported (no runtime params)\n");
                } else {
                    std::fprintf(stderr, "startup error: unknown param '%s'\n", name.c_str());
                }
                return 2;
            }
            continue;
        }
        if (opt == "--duration") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "startup error: --duration requires a value\n");
                return 2;
            }
            std::string d(argv[++i]);
            if (!detail::parse_duration(d, &duration_seconds)) {
                std::fprintf(
                    stderr,
                    "startup error: invalid --duration '%s' (use <sec>, <sec>s, <min>m, or inf)\n",
                    d.c_str());
                return 2;
            }
            continue;
        }
        if (opt == "--threads") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "startup error: --threads requires a positive integer\n");
                return 2;
            }
            try {
                threads = std::stoi(std::string(argv[++i]));
            } catch (...) {
                std::fprintf(stderr, "startup error: --threads requires a positive integer\n");
                return 2;
            }
            if (threads <= 0) {
                std::fprintf(stderr, "startup error: --threads requires a positive integer\n");
                return 2;
            }
            continue;
        }
        if (opt == "--probe") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "startup error: --probe requires a name\n");
                return 2;
            }
            enabled_probes.emplace_back(argv[++i]);
            continue;
        }
        if (opt == "--probe-output") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "startup error: --probe-output requires a path\n");
                return 2;
            }
            probe_output_path = std::string(argv[++i]);
            continue;
        }
        if (opt == "--stats") {
            *desc.state.stats = true;
            continue;
        }
        std::fprintf(stderr, "startup error: unknown option '%s'\n", argv[i]);
        return 2;
    }

    // ── Probe initialization ────────────────────────────────────────────
    // Gate: probes.empty() only (no #ifndef NDEBUG).
    // When probes is empty (release codegen or no probes defined), skip entirely.
    // --probe and --probe-output are silently accepted but have no effect.
    if (!desc.probes.empty()) {
        for (const auto &name : enabled_probes) {
            bool found = false;
            for (const auto &p : desc.probes) {
                if (name == p.name) {
                    *p.enabled = true;
                    found = true;
                    break;
                }
            }
            if (!found) {
                std::fprintf(stderr, "startup error: unknown probe '%s'\n", name.c_str());
                return 2;
            }
        }
        if (!enabled_probes.empty() || probe_output_path != "/dev/stderr") {
            *desc.state.probe_output = std::fopen(probe_output_path.c_str(), "w");
            if (!*desc.state.probe_output) {
                std::fprintf(stderr, "startup error: failed to open probe output file '%s': %s\n",
                             probe_output_path.c_str(), std::strerror(errno));
                return 2;
            }
        }
    }

    // ── Signal handler ──────────────────────────────────────────────────
    static std::atomic<bool> *s_stop = nullptr;
    s_stop = desc.state.stop;
    std::signal(SIGINT, [](int) { s_stop->store(true, std::memory_order_release); });

    // ── Launch task threads ─────────────────────────────────────────────
    std::vector<std::thread> task_threads;
    task_threads.reserve(desc.tasks.size());
    for (const auto &t : desc.tasks) {
        task_threads.emplace_back(t.entry);
    }
    desc.state.start->store(true, std::memory_order_release);

    // ── Duration wait ───────────────────────────────────────────────────
    if (std::isfinite(duration_seconds)) {
        std::this_thread::sleep_for(std::chrono::duration<double>(duration_seconds));
        desc.state.stop->store(true, std::memory_order_release);
    } else {
        // Run until SIGINT
        while (!desc.state.stop->load(std::memory_order_acquire))
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    // ── Join threads ────────────────────────────────────────────────────
    for (auto &t : task_threads) {
        t.join();
    }

    // ── Advisory --threads warning ──────────────────────────────────────
    if (threads > 0 && static_cast<size_t>(threads) < desc.tasks.size()) {
        std::fprintf(stderr, "startup warning: --threads is advisory (requested=%d, tasks=%zu)\n",
                     threads, desc.tasks.size());
    }

    // ── Stats output ────────────────────────────────────────────────────
    if (*desc.state.stats) {
        for (const auto &t : desc.tasks) {
            std::fprintf(stderr,
                         "[stats] task '%s': ticks=%lu, missed=%lu (%s), max_latency=%ldns, "
                         "avg_latency=%ldns\n",
                         t.name, (unsigned long)t.stats->ticks, (unsigned long)t.stats->missed,
                         desc.overrun_policy, t.stats->max_latency_ns, t.stats->avg_latency_ns());
        }
        for (const auto &b : desc.buffers) {
            size_t avail = b.available();
            std::fprintf(stderr, "[stats] shared buffer '%s': %zu tokens (%zuB)\n", b.name, avail,
                         avail * b.elem_size);
        }
        std::fprintf(stderr, "[stats] memory pool: %zuB allocated, %zuB used\n", desc.mem_allocated,
                     desc.mem_used);
    }

    return desc.state.exit_code->load(std::memory_order_acquire);
}

} // namespace pipit
