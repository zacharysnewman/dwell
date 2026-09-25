add_executable(dwell_server main.cpp)
target_link_libraries(dwell_server PRIVATE dwell_core dwell_net dwell_options)
