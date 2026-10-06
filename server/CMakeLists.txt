cmake_minimum_required(VERSION 3.24)
project(dwell_server LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(DWELL_BUILD_TESTS "Build Dwell server unit tests" ON)

include(cmake/Dependencies.cmake)

# The app version (RELEASES.md §3): the one in client/package.json is the next release's; the
# release workflow passes the exact one as DWELL_VERSION (the environment, like the client build).
# Without it this is a local build, a pre-release of the next version. Build metadata ("+sha")
# is dropped: worlds record the version without it.
if(DEFINED ENV{DWELL_VERSION} AND NOT "$ENV{DWELL_VERSION}" STREQUAL "")
  set(_dwell_version "$ENV{DWELL_VERSION}")
else()
  file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../client/package.json" _dwell_package)
  string(JSON _dwell_package_version GET "${_dwell_package}" version)
  set(_dwell_version "${_dwell_package_version}-dev.local")
endif()
string(REGEX REPLACE "\\+.*$" "" DWELL_APP_VERSION "${_dwell_version}")
if(NOT DWELL_APP_VERSION MATCHES "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$")
  message(FATAL_ERROR "not a valid app version: ${DWELL_APP_VERSION}")
endif()
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
             "${CMAKE_CURRENT_SOURCE_DIR}/../client/package.json")
message(STATUS "Dwell app version: ${DWELL_APP_VERSION}")

# Flags for Dwell's own targets. No FMA contraction, to keep native and WASM builds as close as
# possible (PLAYER_CONTROLLER.md §8.3); Jolt applies the same via CROSS_PLATFORM_DETERMINISTIC.
add_library(dwell_options INTERFACE)
if(MSVC)
  target_compile_options(dwell_options INTERFACE /W4 /permissive- /fp:precise)
else()
  target_compile_options(dwell_options INTERFACE -Wall -Wextra -Wpedantic -ffp-contract=off)
endif()

add_subdirectory(core)
add_subdirectory(tools)

if(EMSCRIPTEN)
  # Browser build: the simulation core only (no networking, no native app). ADR 0007.
  add_subdirectory(wasm)
else()
  add_subdirectory(net)
  add_subdirectory(app)
  if(DWELL_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
  endif()
endif()
