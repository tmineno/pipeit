cmake_minimum_required(VERSION 3.20)
project(pipscope LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

include(FetchContent)

# ── GLFW ──────────────────────────────────────────────────────────────────────

FetchContent_Declare(
    glfw
    GIT_REPOSITORY https://github.com/glfw/glfw.git
    GIT_TAG        3.4
    GIT_SHALLOW    TRUE
)
set(GLFW_BUILD_DOCS OFF CACHE BOOL "" FORCE)
set(GLFW_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(GLFW_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(GLFW_INSTALL OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(glfw)

# ── Dear ImGui ────────────────────────────────────────────────────────────────

FetchContent_Declare(
    imgui
    GIT_REPOSITORY https://github.com/ocornut/imgui.git
    GIT_TAG        v1.91.8
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(imgui)

add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_demo.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_glfw.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_opengl3.cpp
)
target_include_directories(imgui PUBLIC
    ${imgui_SOURCE_DIR}
    ${imgui_SOURCE_DIR}/backends
)
target_link_libraries(imgui PUBLIC glfw)

# ── ImPlot ────────────────────────────────────────────────────────────────────

FetchContent_Declare(
    implot
    GIT_REPOSITORY https://github.com/epezent/implot.git
    GIT_TAG        v0.16
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(implot)

add_library(implot STATIC
    ${implot_SOURCE_DIR}/implot.cpp
    ${implot_SOURCE_DIR}/implot_items.cpp
)
target_include_directories(implot PUBLIC ${implot_SOURCE_DIR})
target_link_libraries(implot PUBLIC imgui)

# ── Pipit runtime (header-only) ──────────────────────────────────────────────

set(PIPIT_INCLUDE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../../runtime/libpipit/include)

# ── pipscope executable ──────────────────────────────────────────────────────

find_package(OpenGL REQUIRED)

add_executable(pipscope main.cpp)
target_include_directories(pipscope PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${PIPIT_INCLUDE_DIR}
)
target_link_libraries(pipscope PRIVATE implot OpenGL::GL)
if(UNIX)
    target_link_libraries(pipscope PRIVATE rt)
endif()

# ── E2E tests (headless, no GUI dependencies) ────────────────────────────────

add_executable(test_receiver tests/test_receiver.cpp)
target_include_directories(test_receiver PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${PIPIT_INCLUDE_DIR}
)
target_link_libraries(test_receiver PRIVATE pthread)

add_executable(test_shm_receiver tests/test_shm_receiver.cpp)
target_include_directories(test_shm_receiver PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${PIPIT_INCLUDE_DIR}
)
target_link_libraries(test_shm_receiver PRIVATE pthread)
if(UNIX)
    target_link_libraries(test_shm_receiver PRIVATE rt)
endif()

enable_testing()
add_test(NAME PpktReceiver COMMAND test_receiver)
add_test(NAME ShmReceiver COMMAND test_shm_receiver)
