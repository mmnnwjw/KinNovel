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
find_package(Freetype REQUIRED)
find_package(ZLIB REQUIRED)

# Include directories
include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party
)

# Third-party library target (yyjson)
add_library(kinnovel_thirdparty STATIC
    third_party/yyjson.c
)
target_include_directories(kinnovel_thirdparty PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/third_party)

# Core library
set(CORE_SOURCES
    src/core/Logger.cpp
    src/core/Sha256.cpp
    src/core/Utils.cpp
    src/core/Config.cpp
    src/core/Charset.cpp
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

# Reader library
set(READER_SOURCES
    src/reader/HtmlParser.cpp
    src/reader/WoffNormalizer.cpp
    src/reader/FontEngine.cpp
    src/reader/ReaderDocument.cpp
)
add_library(kinnovel_reader STATIC ${READER_SOURCES})
target_link_libraries(kinnovel_reader PUBLIC
    kinnovel_core
    Freetype::Freetype
    ZLIB::ZLIB
)
target_include_directories(kinnovel_reader PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# Network library
find_package(CURL REQUIRED)
find_package(OpenSSL REQUIRED)

set(NETWORK_SOURCES
    src/network/RateLimiter.cpp
    src/network/HttpTransport.cpp
    src/network/WebSocketClient.cpp
    src/network/SignalRClient.cpp
    src/network/SessionStore.cpp
    src/network/ApiClient.cpp
)
add_library(kinnovel_network STATIC ${NETWORK_SOURCES})
target_link_libraries(kinnovel_network PUBLIC
    kinnovel_core
    CURL::libcurl
    OpenSSL::SSL
    OpenSSL::Crypto
    ZLIB::ZLIB
)
target_include_directories(kinnovel_network PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# UI library
set(UI_SOURCES
    src/ui/Canvas.cpp
    src/ui/ImageCache.cpp
    src/ui/PageContext.cpp
    src/ui/pages/HomePage.cpp
    src/ui/pages/BrowsePage.cpp
    src/ui/pages/RankPage.cpp
    src/ui/pages/BookDetailPage.cpp
    src/ui/pages/SeriesPage.cpp
    src/ui/pages/ShelfPage.cpp
    src/ui/pages/HistoryPage.cpp
    src/ui/pages/ReaderPage.cpp
    src/ui/pages/SettingsPage.cpp
    src/ui/pages/AccountPage.cpp
    src/ui/pages/AnnouncementsPage.cpp
)
add_library(kinnovel_ui STATIC ${UI_SOURCES})
target_link_libraries(kinnovel_ui PUBLIC
    kinnovel_reader
    kinnovel_network
    kinnovel_hal
    kinnovel_core
)
target_include_directories(kinnovel_ui PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# Tools
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/../tools/live_smoke.cpp)
    add_executable(live_smoke ${CMAKE_CURRENT_SOURCE_DIR}/../tools/live_smoke.cpp)
    target_link_libraries(live_smoke PRIVATE kinnovel_network kinnovel_core)
endif()

# Unit & Integration Tests
option(BUILD_TESTS "Build KinNovel unit tests" ON)
if(BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

