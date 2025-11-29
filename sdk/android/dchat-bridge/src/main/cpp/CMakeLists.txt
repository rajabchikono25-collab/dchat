cmake_minimum_required(VERSION 3.22.1)

project("dchat_jni")

# Build the JNI bridge library
add_library(dchat_jni SHARED
    dchat_jni.cpp
)

# Find required packages
find_library(log-lib log)

# Link libraries
target_link_libraries(dchat_jni
    ${log-lib}
)

# Include directories
target_include_directories(dchat_jni PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

# Compiler flags
target_compile_options(dchat_jni PRIVATE
    -Wall
    -Wextra
    -Werror
    -fvisibility=hidden
)

# Release optimizations
if(CMAKE_BUILD_TYPE STREQUAL "Release")
    target_compile_options(dchat_jni PRIVATE
        -O3
        -DNDEBUG
    )
endif()
