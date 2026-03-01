cmake_minimum_required(VERSION 3.16)

if(NOT DEFINED CMAKE_CXX_COMPILER)
    set(CMAKE_CXX_COMPILER clang++)
endif()

project(pipit_examples LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/build)

# ── Options ───────────────────────────────────────────────────────────
# Since v0.4.4, --actor-meta is required for compilation (ADR-033).
# PIPIT_USE_MANIFEST is retained for compatibility but always forced ON.

# ── Locate pcc compiler ──────────────────────────────────────────────
# Prefer PCC variable (set by build.sh), then cargo build output, then PATH.
find_program(PCC pcc
    HINTS ${PCC}
    PATHS ${CMAKE_CURRENT_SOURCE_DIR}/../target/release
          ${CMAKE_CURRENT_SOURCE_DIR}/../target/debug)
if(NOT PCC)
    message(FATAL_ERROR "pcc not found. Build it first: cargo build -p pcc --release")
endif()
message(STATUS "Using pcc: ${PCC}")

# ── Paths ─────────────────────────────────────────────────────────────
set(EXAMPLES_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(RUNTIME_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/../runtime/libpipit/include)

# ── Actor header inventory (explicit, not GLOBbed for DEPENDS) ────────
# Keep this list in sync when adding new actor headers.
set(RUNTIME_ACTOR_HEADERS
    ${RUNTIME_INCLUDE}/pipit.h
    ${RUNTIME_INCLUDE}/pipit_bind_io.h
    ${RUNTIME_INCLUDE}/pipit_net.h
    ${RUNTIME_INCLUDE}/pipit_shell.h
    ${RUNTIME_INCLUDE}/pipit_shm.h
    ${RUNTIME_INCLUDE}/std_actors.h
    ${RUNTIME_INCLUDE}/std_math.h
    ${RUNTIME_INCLUDE}/std_sink.h
    ${RUNTIME_INCLUDE}/std_source.h
)

set(EXAMPLE_ACTOR_HEADERS
    ${EXAMPLES_DIR}/example_actors.h
    ${EXAMPLES_DIR}/poly_actors.h
    ${EXAMPLES_DIR}/shm/shm_actors.h
)

set(ALL_ACTOR_HEADERS ${RUNTIME_ACTOR_HEADERS} ${EXAMPLE_ACTOR_HEADERS})

# ── Configure-time cross-check: warn if unlisted headers exist ────────
# Scoped to known actor header patterns (excludes third_party/).
file(GLOB _runtime_glob
    ${RUNTIME_INCLUDE}/std_*.h
    ${RUNTIME_INCLUDE}/pipit*.h
)
file(GLOB _example_glob
    ${EXAMPLES_DIR}/*_actors.h
    ${EXAMPLES_DIR}/poly_*.h
    ${EXAMPLES_DIR}/shm/*_actors.h
)
set(_all_glob ${_runtime_glob} ${_example_glob})
foreach(_h IN LISTS _all_glob)
    list(FIND ALL_ACTOR_HEADERS "${_h}" _idx)
    if(_idx EQUAL -1)
        message(WARNING "Actor header not in ALL_ACTOR_HEADERS: ${_h}")
    endif()
endforeach()

# ── Manifest generation (required since v0.4.4) ──────────────────────
set(ACTOR_MANIFEST ${CMAKE_CURRENT_BINARY_DIR}/actors.meta.json)

add_custom_command(
    OUTPUT  ${ACTOR_MANIFEST}
    COMMAND ${PCC} --emit manifest
            -I ${RUNTIME_INCLUDE} -I ${EXAMPLES_DIR} -I ${EXAMPLES_DIR}/shm
            -o ${ACTOR_MANIFEST}
    DEPENDS ${ALL_ACTOR_HEADERS} ${PCC}
    COMMENT "pcc: generating actors.meta.json from actor headers"
)

add_custom_target(actor_manifest DEPENDS ${ACTOR_MANIFEST})

# ── PDL → C++ → binary function ──────────────────────────────────────
function(add_pdl_example PDL_NAME)
    set(PDL_FILE  ${EXAMPLES_DIR}/${PDL_NAME}.pdl)
    set(CPP_FILE  ${CMAKE_CURRENT_BINARY_DIR}/${PDL_NAME}.cpp)

    add_custom_command(
        OUTPUT  ${CPP_FILE}
        COMMAND ${PCC} ${PDL_FILE}
                --actor-meta ${ACTOR_MANIFEST}
                -I ${EXAMPLES_DIR} -I ${RUNTIME_INCLUDE}
                --emit cpp -o ${CPP_FILE}
        DEPENDS ${PDL_FILE} ${ACTOR_MANIFEST} ${PCC}
        COMMENT "pcc: ${PDL_NAME}.pdl -> ${PDL_NAME}.cpp"
    )

    # Compile generated C++ to binary
    add_executable(${PDL_NAME} ${CPP_FILE})
    add_dependencies(${PDL_NAME} actor_manifest)
    target_include_directories(${PDL_NAME} PRIVATE
        ${RUNTIME_INCLUDE}
        ${RUNTIME_INCLUDE}/third_party
        ${EXAMPLES_DIR}
    )
    target_link_libraries(${PDL_NAME} PRIVATE pthread)
endfunction()

# ── PDL → C++ → binary (subdirectory variant) ────────────────────────
# For examples in subdirectories like shm/writer.pdl.
# Links additional libraries (e.g. rt for POSIX shm).
function(add_pdl_subdir_example SUBDIR PDL_NAME)
    set(PDL_FILE  ${EXAMPLES_DIR}/${SUBDIR}/${PDL_NAME}.pdl)
    set(CPP_FILE  ${CMAKE_CURRENT_BINARY_DIR}/${SUBDIR}_${PDL_NAME}.cpp)
    set(TARGET_NAME ${SUBDIR}_${PDL_NAME})

    add_custom_command(
        OUTPUT  ${CPP_FILE}
        COMMAND ${PCC} ${PDL_FILE}
                --actor-meta ${ACTOR_MANIFEST}
                -I ${EXAMPLES_DIR} -I ${EXAMPLES_DIR}/${SUBDIR} -I ${RUNTIME_INCLUDE}
                --emit cpp -o ${CPP_FILE}
        DEPENDS ${PDL_FILE} ${ACTOR_MANIFEST} ${PCC}
        COMMENT "pcc: ${SUBDIR}/${PDL_NAME}.pdl -> ${TARGET_NAME}.cpp"
    )

    add_executable(${TARGET_NAME} ${CPP_FILE})
    add_dependencies(${TARGET_NAME} actor_manifest)
    target_include_directories(${TARGET_NAME} PRIVATE
        ${RUNTIME_INCLUDE}
        ${RUNTIME_INCLUDE}/third_party
        ${EXAMPLES_DIR}
        ${EXAMPLES_DIR}/${SUBDIR}
    )
    target_link_libraries(${TARGET_NAME} PRIVATE pthread ${ARGN})
endfunction()

# ── Examples ──────────────────────────────────────────────────────────
add_pdl_example(gain)
add_pdl_example(example)
add_pdl_example(complex)
add_pdl_example(receiver)
add_pdl_example(feedback)
add_pdl_example(multirate)
add_pdl_example(socket_stream)
add_pdl_example(bind)
add_pdl_example(multichannel)

# ── SHM examples (require -lrt on Linux for shm_open) ────────────────
add_pdl_subdir_example(shm writer rt)
add_pdl_subdir_example(shm reader rt)