# Local-mode build of the server core for browsers (ARCHITECTURE.md §2.1, ADR 0007: single-threaded).
# Produces dwell_core.js + dwell_core.wasm (ES module factory) for client/public/wasm.
add_executable(dwell_core_wasm wasm_api.cpp)
target_link_libraries(dwell_core_wasm PRIVATE dwell_core dwell_options)
set_target_properties(dwell_core_wasm PROPERTIES OUTPUT_NAME dwell_core SUFFIX ".js")
target_link_options(dwell_core_wasm PRIVATE
  -sMODULARIZE=1
  -sEXPORT_ES6=1
  -sEXPORT_NAME=createDwellCore
  -sENVIRONMENT=web,worker,node
  -sALLOW_MEMORY_GROWTH=1
  -sINITIAL_MEMORY=64MB
  -sSTACK_SIZE=1MB
  -sEXPORTED_FUNCTIONS=_malloc,_free
  -sEXPORTED_RUNTIME_METHODS=HEAPU8,HEAPU16,HEAP32,HEAPU32,HEAPF64
  -sFILESYSTEM=0)

# Copy into the client so Vite serves it at <base>/wasm/.
set(DWELL_WASM_OUT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../client/public/wasm" CACHE PATH
    "Where to copy the local-mode WASM build")
add_custom_command(TARGET dwell_core_wasm POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E make_directory "${DWELL_WASM_OUT_DIR}"
  COMMAND ${CMAKE_COMMAND} -E copy
          "$<TARGET_FILE_DIR:dwell_core_wasm>/dwell_core.js"
          "$<TARGET_FILE_DIR:dwell_core_wasm>/dwell_core.wasm"
          "${DWELL_WASM_OUT_DIR}")

# The terrain generator alone for the client's worldgen workers (§5.1, §6.3): dwell_worldgen.js +
# .wasm. Links only the generator's objects from dwell_core (no Jolt), with a small heap per worker.
add_executable(dwell_worldgen_wasm worldgen_api.cpp)
target_link_libraries(dwell_worldgen_wasm PRIVATE dwell_core dwell_options)
set_target_properties(dwell_worldgen_wasm PROPERTIES OUTPUT_NAME dwell_worldgen SUFFIX ".js")
target_link_options(dwell_worldgen_wasm PRIVATE
  -sMODULARIZE=1
  -sEXPORT_ES6=1
  -sEXPORT_NAME=createDwellWorldgen
  -sENVIRONMENT=web,worker,node
  -sALLOW_MEMORY_GROWTH=1
  -sINITIAL_MEMORY=4MB
  -sSTACK_SIZE=1MB
  -sEXPORTED_FUNCTIONS=_malloc,_free
  -sEXPORTED_RUNTIME_METHODS=HEAPU8,HEAPU16,HEAPU32
  -sFILESYSTEM=0)
add_custom_command(TARGET dwell_worldgen_wasm POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E make_directory "${DWELL_WASM_OUT_DIR}"
  COMMAND ${CMAKE_COMMAND} -E copy
          "$<TARGET_FILE_DIR:dwell_worldgen_wasm>/dwell_worldgen.js"
          "$<TARGET_FILE_DIR:dwell_worldgen_wasm>/dwell_worldgen.wasm"
          "${DWELL_WASM_OUT_DIR}")
