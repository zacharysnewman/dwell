add_executable(dwell_tests
  main.cpp
  jolt_smoke_test.cpp
  net_abi_test.cpp)
target_link_libraries(dwell_tests PRIVATE dwell_core dwell_net dwell_options doctest::doctest)
add_test(NAME dwell_tests COMMAND dwell_tests)
