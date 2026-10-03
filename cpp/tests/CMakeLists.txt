include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/..
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
    ${CMAKE_CURRENT_SOURCE_DIR}/../third_party
)

add_executable(test_hal
    unit/test_hal.cpp
)
target_link_libraries(test_hal PRIVATE kinnovel_hal kinnovel_core)

add_test(NAME test_hal COMMAND test_hal)
