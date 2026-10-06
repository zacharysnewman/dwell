add_executable(dwell_server main.cpp)
target_link_libraries(dwell_server PRIVATE dwell_core dwell_net dwell_options)

# The licence notices ship beside the native server (RELEASES.md §2): copied next to the binary
# after each build, and installed with it.
set(DWELL_NOTICES "${CMAKE_CURRENT_SOURCE_DIR}/../../THIRD_PARTY_NOTICES"
                  "${CMAKE_CURRENT_SOURCE_DIR}/../../LICENSE")
add_custom_command(TARGET dwell_server POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different ${DWELL_NOTICES} "$<TARGET_FILE_DIR:dwell_server>"
  COMMENT "Copying THIRD_PARTY_NOTICES and LICENSE beside dwell_server")
install(TARGETS dwell_server RUNTIME DESTINATION .)
install(FILES ${DWELL_NOTICES} DESTINATION .)
