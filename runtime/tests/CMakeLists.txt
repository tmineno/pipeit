cmake_minimum_required(VERSION 3.14)
project(pipit_runtime_tests)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Include directories
include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/../libpipit/include
    ${CMAKE_CURRENT_SOURCE_DIR}/../libpipit/include/third_party
)

# Test files
add_executable(test_arithmetic test_arithmetic.cpp)
add_executable(test_statistics test_statistics.cpp)
add_executable(test_fft test_fft.cpp)
add_executable(test_transform test_transform.cpp)
add_executable(test_utility test_utility.cpp)
add_executable(test_timer test_timer.cpp)
add_executable(test_waveform test_waveform.cpp)
add_executable(test_net test_net.cpp)
add_executable(test_socket_actors test_socket_actors.cpp)
add_executable(test_shell test_shell.cpp)
add_executable(test_ringbuf test_ringbuf.cpp)

# Enable testing
enable_testing()

add_test(NAME ArithmeticActors COMMAND test_arithmetic)
add_test(NAME StatisticsActors COMMAND test_statistics)
add_test(NAME FFTActor COMMAND test_fft)
add_test(NAME TransformActors COMMAND test_transform)
add_test(NAME UtilityActors COMMAND test_utility)
add_test(NAME TimerAdaptive COMMAND test_timer)
add_test(NAME WaveformActors COMMAND test_waveform)
add_test(NAME PpktNet COMMAND test_net)
add_test(NAME SocketActors COMMAND test_socket_actors)
add_test(NAME Shell COMMAND test_shell)
add_test(NAME RingBuffer COMMAND test_ringbuf)
