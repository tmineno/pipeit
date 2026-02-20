cmake_minimum_required(VERSION 3.16)
project(libpipit VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(pipit INTERFACE)
target_include_directories(pipit INTERFACE include/ include/third_party/)
