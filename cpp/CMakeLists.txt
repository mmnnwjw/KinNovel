cmake_minimum_required(VERSION 3.16)
project(kinnovel VERSION 0.6.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Export compile commands for language servers
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# Global compiler flags
if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    add_compile_options(-Wall -Wextra -Wpedantic -Wno-unused-parameter)
endif()

# Find basic dependencies
find_package(Threads REQUIRED)

# Include directories
include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party
)

# Optional system packages for native/cross builds
find_package(PkgConfig QUIET)
if(PKG_CONFIG_FOUND)
    pkg_check_modules(FREETYPE QUIET freetype2)
    pkg_check_modules(CURL QUIET libcurl)
    pkg_check_modules(ZLIB QUIET zlib)
    pkg_check_modules(BROTLI QUIET libbrotlidec)
endif()

# Third-party library target (yyjson)
add_library(kinnovel_thirdparty STATIC
    third_party/yyjson.c
)
target_include_directories(kinnovel_thirdparty PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/third_party)

# Core library
set(CORE_SOURCES
    src/core/Logger.cpp
)
add_library(kinnovel_core STATIC ${CORE_SOURCES})
target_link_libraries(kinnovel_core PUBLIC kinnovel_thirdparty Threads::Threads)
target_include_directories(kinnovel_core PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# HAL library (Hardware Abstraction Layer)
set(HAL_SOURCES
    src/hal/mock/MockDisplay.cpp
    src/hal/mock/MockInput.cpp
)

if(CMAKE_SYSTEM_NAME MATCHES "Linux|Android")
    list(APPEND HAL_SOURCES
        src/hal/kindle/FbinkDisplay.cpp
        src/hal/kindle/EvdevInput.cpp
        src/hal/kindle/LipcPowerManager.cpp
    )
endif()

add_library(kinnovel_hal STATIC ${HAL_SOURCES})
target_link_libraries(kinnovel_hal PUBLIC kinnovel_core)
target_include_directories(kinnovel_hal PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# Unit & Integration Tests
option(BUILD_TESTS "Build KinNovel unit tests" ON)
if(BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
