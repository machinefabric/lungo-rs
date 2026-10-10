# The codec example from C: builds the generated package (EXAMPLE_PACKAGE) with every warning as
# an error, and a test program using its API.
cmake_minimum_required(VERSION 3.20)
project(codec_c_test C)

add_subdirectory(${EXAMPLE_PACKAGE} varint)
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(varint PRIVATE -Wall -Wextra -Werror)
elseif(MSVC)
  target_compile_options(varint PRIVATE /W3 /WX)
endif()

add_executable(codec_test test.c)
target_link_libraries(codec_test PRIVATE varint)
