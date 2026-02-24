#pragma once
/// @file cli.h
/// @brief CLI argument parsing for pipscope (inline, no .cpp needed)

#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace pipscope {

struct CliArgs {
    char address[128] = {};
    bool has_address = false;
    bool error = false;
    bool vsync = false;
    int snapshot_hz = 0;
};

inline void print_usage(const char *argv0) {
    fprintf(stderr, "Usage: %s [--port <port>] [--address <addr>] [options]\n", argv0);
    fprintf(stderr, "       %s [-p <port>] [-a <addr>] [options]\n", argv0);
    fprintf(stderr, "\n");
    fprintf(stderr, "  -p, --port <port>       Listen on 0.0.0.0:<port> (UDP)\n");
    fprintf(stderr, "  -a, --address <addr>    Listen on <addr> (e.g. localhost:9100)\n");
    fprintf(stderr, "      --vsync             Enable vsync (default: off)\n");
    fprintf(stderr, "      --snapshot-hz <N>   Limit snapshot rate to N Hz (0 = unlimited)\n");
    fprintf(stderr, "  -h, --help              Show this help message\n");
    fprintf(stderr, "\nIf no address is given, starts with GUI address input.\n");
}

inline CliArgs parse_args(int argc, char **argv) {
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
        if (strcmp(argv[i], "--vsync") == 0) {
            args.vsync = true;
            continue;
        }
        if (strcmp(argv[i], "--snapshot-hz") == 0 && i + 1 < argc) {
            args.snapshot_hz = atoi(argv[i + 1]);
            i++;
            continue;
        }
    }
    return args;
}

} // namespace pipscope
