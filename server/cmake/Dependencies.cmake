include(FetchContent)

# --- Jolt Physics ------------------------------------------------------------------------------
set(TARGET_UNIT_TESTS OFF CACHE BOOL "" FORCE)
set(TARGET_HELLO_WORLD OFF CACHE BOOL "" FORCE)
set(TARGET_PERFORMANCE_TEST OFF CACHE BOOL "" FORCE)
set(TARGET_SAMPLES OFF CACHE BOOL "" FORCE)
set(TARGET_VIEWER OFF CACHE BOOL "" FORCE)
set(ENABLE_ALL_WARNINGS OFF CACHE BOOL "" FORCE)
set(INTERPROCEDURAL_OPTIMIZATION OFF CACHE BOOL "" FORCE)
set(OVERRIDE_CXX_FLAGS OFF CACHE BOOL "" FORCE)
set(DEBUG_RENDERER_IN_DEBUG_AND_RELEASE OFF CACHE BOOL "" FORCE)
set(PROFILER_IN_DEBUG_AND_RELEASE OFF CACHE BOOL "" FORCE)
# Minimize native/WASM divergence (PLAYER_CONTROLLER.md §8.3).
set(CROSS_PLATFORM_DETERMINISTIC ON CACHE BOOL "" FORCE)

FetchContent_Declare(
  JoltPhysics
  GIT_REPOSITORY https://github.com/jrouwe/JoltPhysics.git
  GIT_TAG v5.6.0
  GIT_SHALLOW TRUE
  SOURCE_SUBDIR Build)

# --- Corrosion (Cargo ↔ CMake, ADR 0001) ------------------------------------------------------
FetchContent_Declare(
  Corrosion
  GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
  GIT_TAG v0.6.1
  GIT_SHALLOW TRUE)

FetchContent_MakeAvailable(JoltPhysics)
if(NOT EMSCRIPTEN)
  FetchContent_MakeAvailable(Corrosion)
endif()

# --- Monocypher (Ed25519 signature checks for device-key identity, ADR 0004) --------------------
# Small, portable C with no build system of its own; compiles unchanged to WASM.
FetchContent_Declare(
  Monocypher
  GIT_REPOSITORY https://github.com/LoupVaillant/Monocypher.git
  GIT_TAG 4.0.3
  GIT_SHALLOW TRUE)
FetchContent_MakeAvailable(Monocypher)
add_library(monocypher STATIC
  ${monocypher_SOURCE_DIR}/src/monocypher.c
  ${monocypher_SOURCE_DIR}/src/optional/monocypher-ed25519.c)
target_include_directories(monocypher SYSTEM PUBLIC
  ${monocypher_SOURCE_DIR}/src ${monocypher_SOURCE_DIR}/src/optional)

# --- doctest ------------------------------------------------------------------------------------
# Native unit tests, and the player test suite built to WASM (PLAYER_CONTROLLER.md §8.3).
if(DWELL_BUILD_TESTS OR EMSCRIPTEN)
  FetchContent_Declare(
    doctest
    GIT_REPOSITORY https://github.com/doctest/doctest.git
    GIT_TAG v2.5.3
    GIT_SHALLOW TRUE)
  set(DOCTEST_NO_INSTALL ON CACHE BOOL "" FORCE)
  FetchContent_MakeAvailable(doctest)
endif()
