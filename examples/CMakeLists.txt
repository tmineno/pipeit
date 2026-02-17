cmake_minimum_required(VERSION 3.16)
project(pipit_examples LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/build)

# ── Locate pcc compiler ─────────────────────────────────────────────────
# Prefer PCC variable, then search PATH, then cargo build output.
find_program(PCC pcc
    HINTS ${PCC}
    PATHS ${CMAKE_CURRENT_SOURCE_DIR}/../target/release
          ${CMAKE_CURRENT_SOURCE_DIR}/../target/debug)
if(NOT PCC)
    message(FATAL_ERROR "pcc not found. Build it first: cargo build -p pcc --release")
endif()
message(STATUS "Using pcc: ${PCC}")

# ── Paths ────────────────────────────────────────────────────────────────
set(EXAMPLES_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(RUNTIME_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/../runtime/libpipit/include)

# ── PDL → C++ → binary function ─────────────────────────────────────────
function(add_pdl_example PDL_NAME)
    set(PDL_FILE  ${EXAMPLES_DIR}/${PDL_NAME}.pdl)
    set(CPP_FILE  ${CMAKE_CURRENT_BINARY_DIR}/${PDL_NAME}.cpp)

    # Generate C++ from PDL
    add_custom_command(
        OUTPUT  ${CPP_FILE}
        COMMAND ${PCC} ${PDL_FILE} -I ${EXAMPLES_DIR} -I ${RUNTIME_INCLUDE} --emit cpp -o ${CPP_FILE}
        DEPENDS ${PDL_FILE} ${EXAMPLES_DIR}/example_actors.h
        COMMENT "pcc: ${PDL_NAME}.pdl -> ${PDL_NAME}.cpp"
    )

    # Compile generated C++ to binary
    add_executable(${PDL_NAME} ${CPP_FILE})
    target_include_directories(${PDL_NAME} PRIVATE ${RUNTIME_INCLUDE} ${EXAMPLES_DIR})
    target_link_libraries(${PDL_NAME} PRIVATE pthread)
endfunction()

# ── Examples ─────────────────────────────────────────────────────────────
add_pdl_example(gain)
add_pdl_example(example)
add_pdl_example(complex)
add_pdl_example(receiver)
add_pdl_example(feedback)
add_pdl_example(multirate)
add_pdl_example(socket_stream)
