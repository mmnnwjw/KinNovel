# CMake toolchain file for Kindle armhf (kindlehf) using koxtoolchain
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR arm)

set(TOOLCHAIN_PREFIX arm-kindlehf-linux-gnueabihf)
if(NOT DEFINED CMAKE_C_COMPILER)
    set(CMAKE_C_COMPILER ${TOOLCHAIN_PREFIX}-gcc)
endif()
if(NOT DEFINED CMAKE_CXX_COMPILER)
    set(CMAKE_CXX_COMPILER ${TOOLCHAIN_PREFIX}-g++)
endif()

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# Kindle hardware flags (Cortex-A7 / Cortex-A9 / Cortex-A53 32-bit with NEON VFPv4)
add_compile_options(
    -march=armv7-a
    -mfpu=neon-vfpv4
    -mfloat-abi=hard
    -O2
    -ffunction-sections
    -fdata-sections
)

add_link_options(
    -static-libgcc
    -static-libstdc++
    -Wl,--gc-sections
)
