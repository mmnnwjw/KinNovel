include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/..
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
    ${CMAKE_CURRENT_SOURCE_DIR}/../third_party
)

# HAL Test
add_executable(test_hal
    unit/test_hal.cpp
)
target_link_libraries(test_hal PRIVATE kinnovel_hal kinnovel_core)
add_test(NAME test_hal COMMAND test_hal)

# Core Test
add_executable(test_core
    unit/test_core.cpp
)
target_link_libraries(test_core PRIVATE kinnovel_core)
add_test(NAME test_core COMMAND test_core)

# Reader Unit Test
add_executable(test_reader
    unit/test_reader.cpp
)
target_link_libraries(test_reader PRIVATE kinnovel_reader kinnovel_core)
add_test(NAME test_reader COMMAND test_reader)

# Reader Golden Test
add_executable(test_golden
    unit/test_golden.cpp
)
target_link_libraries(test_golden PRIVATE kinnovel_reader kinnovel_core)
add_test(NAME test_golden COMMAND test_golden)
