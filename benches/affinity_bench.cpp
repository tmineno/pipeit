// CPU affinity impact benchmarks
//
// Measures how thread-to-core pinning affects ring buffer throughput.
// Probes CPU topology at startup from sysfs to select representative
// CPU pairs (SMT siblings, adjacent cores, distant cores).

#include <atomic>
#include <benchmark/benchmark.h>
#include <cstdio>
#include <fstream>
#include <map>
#include <pipit.h>
#include <sched.h>
#include <string>
#include <thread>
#include <vector>

using namespace pipit;

// ── Topology probing ────────────────────────────────────────────────────

struct CpuPair {
    int a;
    int b;
};

struct Topology {
    CpuPair smt;                         // same physical core (SMT siblings)
    CpuPair near_pair;                   // adjacent physical cores
    CpuPair far_pair;                    // most-distant physical cores
    std::vector<int> physical_core_cpus; // one CPU per physical core
    bool valid = false;
};

static Topology g_topo;

static std::string read_sysfs(const std::string &path) {
    std::ifstream f(path);
    std::string val;
    if (f.is_open())
        std::getline(f, val);
    return val;
}

static void probe_topology() {
    // Map core_id -> list of CPUs
    std::map<int, std::vector<int>> core_to_cpus;
    std::vector<int> core_order;

    int max_cpu = static_cast<int>(std::thread::hardware_concurrency());
    for (int cpu = 0; cpu < max_cpu; ++cpu) {
        std::string base = "/sys/devices/system/cpu/cpu" + std::to_string(cpu);
        std::string core_str = read_sysfs(base + "/topology/core_id");
        if (core_str.empty())
            continue;
        int core_id = std::stoi(core_str);
        if (core_to_cpus.find(core_id) == core_to_cpus.end())
            core_order.push_back(core_id);
        core_to_cpus[core_id].push_back(cpu);
    }

    if (core_order.empty())
        return;

    // Physical core representative CPUs (first CPU per core)
    for (int cid : core_order)
        g_topo.physical_core_cpus.push_back(core_to_cpus[cid][0]);

    // SMT pair: first core with 2+ CPUs
    g_topo.smt = {g_topo.physical_core_cpus[0], g_topo.physical_core_cpus[0]};
    for (int cid : core_order) {
        if (core_to_cpus[cid].size() >= 2) {
            g_topo.smt = {core_to_cpus[cid][0], core_to_cpus[cid][1]};
            break;
        }
    }

    // Adjacent: first two distinct physical cores
    g_topo.near_pair = {g_topo.physical_core_cpus[0], g_topo.physical_core_cpus.size() > 1
                                                          ? g_topo.physical_core_cpus[1]
                                                          : g_topo.physical_core_cpus[0]};

    // Distant: first and last physical cores
    g_topo.far_pair = {g_topo.physical_core_cpus.front(), g_topo.physical_core_cpus.back()};

    g_topo.valid = true;

    fprintf(stderr,
            "[affinity] topology: smt=(%d,%d) near=(%d,%d) far=(%d,%d) "
            "physical_cores=%zu\n",
            g_topo.smt.a, g_topo.smt.b, g_topo.near_pair.a, g_topo.near_pair.b, g_topo.far_pair.a,
            g_topo.far_pair.b, g_topo.physical_core_cpus.size());
}

// ── Helpers ──────────────────────────────────────────────────────────────

static bool pin_to_cpu(int cpu) {
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(cpu, &set);
    return sched_setaffinity(0, sizeof(set), &set) == 0;
}

// Ring buffer throughput template with optional writer/reader pinning
static void run_affinity_bench(benchmark::State &state, int writer_cpu, int reader_cpu) {
    static constexpr std::size_t CAP = 4096;
    static constexpr std::size_t CHUNK = 64;

    for (auto _ : state) {
        RingBuffer<float, CAP, 1> rb;
        float write_data[CHUNK];
        for (std::size_t i = 0; i < CHUNK; ++i)
            write_data[i] = static_cast<float>(i);

        std::atomic<bool> done{false};
        std::atomic<uint64_t> read_count{0};

        std::thread reader([&] {
            if (reader_cpu >= 0)
                pin_to_cpu(reader_cpu);
            float buf[CHUNK];
            while (!done.load(std::memory_order_acquire)) {
                if (rb.read(0, buf, CHUNK))
                    read_count.fetch_add(CHUNK, std::memory_order_relaxed);
            }
            while (rb.read(0, buf, CHUNK))
                read_count.fetch_add(CHUNK, std::memory_order_relaxed);
        });

        if (writer_cpu >= 0)
            pin_to_cpu(writer_cpu);

        uint64_t written = 0;
        const uint64_t target = 500'000;
        while (written < target) {
            if (rb.write(write_data, CHUNK))
                written += CHUNK;
        }

        done.store(true, std::memory_order_release);
        reader.join();

        state.SetItemsProcessed(written);
        state.SetBytesProcessed(written * sizeof(float));
    }
}

// ── Benchmarks ──────────────────────────────────────────────────────────

static void BM_Affinity_Unpinned(benchmark::State &state) { run_affinity_bench(state, -1, -1); }
BENCHMARK(BM_Affinity_Unpinned)->Unit(benchmark::kMillisecond);

static void BM_Affinity_SameCore(benchmark::State &state) {
    if (!g_topo.valid) {
        state.SkipWithError("topology probing failed");
        return;
    }
    run_affinity_bench(state, g_topo.smt.a, g_topo.smt.b);
}
BENCHMARK(BM_Affinity_SameCore)->Unit(benchmark::kMillisecond);

static void BM_Affinity_AdjacentCore(benchmark::State &state) {
    if (!g_topo.valid) {
        state.SkipWithError("topology probing failed");
        return;
    }
    run_affinity_bench(state, g_topo.near_pair.a, g_topo.near_pair.b);
}
BENCHMARK(BM_Affinity_AdjacentCore)->Unit(benchmark::kMillisecond);

static void BM_Affinity_DistantCore(benchmark::State &state) {
    if (!g_topo.valid) {
        state.SkipWithError("topology probing failed");
        return;
    }
    run_affinity_bench(state, g_topo.far_pair.a, g_topo.far_pair.b);
}
BENCHMARK(BM_Affinity_DistantCore)->Unit(benchmark::kMillisecond);

// ── Task scaling with pinning ───────────────────────────────────────────

static void BM_Affinity_TaskScaling(benchmark::State &state) {
    if (!g_topo.valid) {
        state.SkipWithError("topology probing failed");
        return;
    }
    const int n_threads = static_cast<int>(state.range(0));
    const int ticks_per_thread = 100;
    const double freq = 10000.0;

    for (auto _ : state) {
        std::atomic<uint64_t> total_ticks{0};
        std::atomic<uint64_t> total_missed{0};
        std::vector<std::thread> threads;

        for (int t = 0; t < n_threads; ++t) {
            threads.emplace_back([&, t, freq] {
                // Pin to one CPU per physical core (wrap if n_threads > cores)
                int cpu_idx = t % static_cast<int>(g_topo.physical_core_cpus.size());
                pin_to_cpu(g_topo.physical_core_cpus[cpu_idx]);

                Timer timer(freq);
                TaskStats stats;
                for (int i = 0; i < ticks_per_thread; ++i) {
                    timer.wait();
                    if (timer.overrun())
                        stats.record_miss();
                    else
                        stats.record_tick(timer.last_latency());
                }
                total_ticks.fetch_add(stats.ticks, std::memory_order_relaxed);
                total_missed.fetch_add(stats.missed, std::memory_order_relaxed);
            });
        }

        for (auto &t : threads)
            t.join();

        state.SetItemsProcessed(total_ticks.load(std::memory_order_relaxed) +
                                total_missed.load(std::memory_order_relaxed));
    }
}
BENCHMARK(BM_Affinity_TaskScaling)
    ->Arg(1)
    ->Arg(2)
    ->Arg(4)
    ->Arg(8)
    ->Arg(16)
    ->Arg(32)
    ->Unit(benchmark::kMillisecond);

// ── Main ─────────────────────────────────────────────────────────────────

int main(int argc, char **argv) {
    probe_topology();
    benchmark::Initialize(&argc, argv);
    benchmark::RunSpecifiedBenchmarks();
    benchmark::Shutdown();
    return 0;
}
