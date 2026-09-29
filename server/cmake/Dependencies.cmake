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
# Planet-scale world (ADR 0011): body positions are doubles (RVec3); shapes and velocities stay float.
set(DOUBLE_PRECISION ON CACHE BOOL "" FORCE)
# RTTI, so Dwell can subclass Jolt interfaces whose key functions live in the library (GroupFilter:
# terrain collision groups, core/terrain_collision.cpp).
set(CPP_RTTI_ENABLED ON CACHE BOOL "" FORCE)

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

# --- SQLite (world persistence, ADR 0006) -------------------------------------------------------
# The official amalgamation, compiled into the core natively and to WASM. Offline builds can point
# FETCHCONTENT_SOURCE_DIR_SQLITE3 at a directory holding sqlite3.c / sqlite3.h of this version.
FetchContent_Declare(
  sqlite3
  URL https://www.sqlite.org/2026/sqlite-amalgamation-3530400.zip
  DOWNLOAD_EXTRACT_TIMESTAMP TRUE)
FetchContent_MakeAvailable(sqlite3)
add_library(dwell_sqlite STATIC ${sqlite3_SOURCE_DIR}/sqlite3.c)
target_include_directories(dwell_sqlite SYSTEM PUBLIC ${sqlite3_SOURCE_DIR})
target_compile_definitions(dwell_sqlite PUBLIC
  SQLITE_DQS=0
  SQLITE_DEFAULT_MEMSTATUS=0
  SQLITE_LIKE_DOESNT_MATCH_BLOBS
  SQLITE_OMIT_DEPRECATED
  SQLITE_OMIT_LOAD_EXTENSION
  SQLITE_OMIT_SHARED_CACHE)
if(EMSCRIPTEN)
  # Single-threaded (ADR 0007), no built-in OS layer: the core registers its own VFS over OPFS
  # (server/wasm/opfs_vfs.cpp) from sqlite3_os_init.
  target_compile_definitions(dwell_sqlite PUBLIC SQLITE_THREADSAFE=0 SQLITE_OS_OTHER=1
                             SQLITE_TEMP_STORE=3)
else()
  # One connection per thread (the tick's reads, the I/O thread's saves).
  target_compile_definitions(dwell_sqlite PUBLIC SQLITE_THREADSAFE=2)
  find_package(Threads REQUIRED)
  target_link_libraries(dwell_sqlite PUBLIC Threads::Threads ${CMAKE_DL_LIBS})
endif()
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(dwell_sqlite PRIVATE -w)
endif()

# --- zstd (world file chunk compression, §6.4) ---------------------------------------------------
FetchContent_Declare(
  zstd
  GIT_REPOSITORY https://github.com/facebook/zstd.git
  GIT_TAG v1.5.7
  GIT_SHALLOW TRUE
  SOURCE_SUBDIR build/does-not-exist)  # sources only: built below without zstd's own CMake
FetchContent_MakeAvailable(zstd)
file(GLOB DWELL_ZSTD_SOURCES
  ${zstd_SOURCE_DIR}/lib/common/*.c
  ${zstd_SOURCE_DIR}/lib/compress/*.c
  ${zstd_SOURCE_DIR}/lib/decompress/*.c)
add_library(dwell_zstd STATIC ${DWELL_ZSTD_SOURCES})
target_include_directories(dwell_zstd SYSTEM PUBLIC ${zstd_SOURCE_DIR}/lib)
# Portable C only (no x86 assembly), single-threaded (ZSTD_MULTITHREAD undefined): the same code
# natively and in WASM.
target_compile_definitions(dwell_zstd PRIVATE ZSTD_DISABLE_ASM)
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(dwell_zstd PRIVATE -w)
endif()
