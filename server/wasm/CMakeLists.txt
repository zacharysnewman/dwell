# Local-mode build of the server core for browsers (ARCHITECTURE.md §2.1, ADR 0007: single-threaded).
# Produces dwell_core.js + dwell_core.wasm (ES module factory) for client/public/wasm.
add_executable(dwell_core_wasm wasm_api.cpp)
target_link_libraries(dwell_core_wasm PRIVATE dwell_core dwell_options)
set_target_properties(dwell_core_wasm PROPERTIES OUTPUT_NAME dwell_core SUFFIX ".js")
target_link_options(dwell_core_wasm PRIVATE
  -sMODULARIZE=1
  -sEXPORT_ES6=1
  -sEXPORT_NAME=createDwellCore
  -sENVIRONMENT=worker,node
  -sALLOW_MEMORY_GROWTH=1
  -sINITIAL_MEMORY=64MB
  -sEXPORTED_FUNCTIONS=_malloc,_free
  -sEXPORTED_RUNTIME_METHODS=HEAPU8,HEAPU32
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
