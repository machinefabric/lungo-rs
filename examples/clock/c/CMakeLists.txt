# The clock example from C: builds the generated package (EXAMPLE_PACKAGE) with every warning as
# an error, and a test program implementing the clock facility.
cmake_minimum_required(VERSION 3.20)
project(clock_c_test C)

add_subdirectory(${EXAMPLE_PACKAGE} timing)
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(timing PRIVATE -Wall -Wextra -Werror)
elseif(MSVC)
  target_compile_options(timing PRIVATE /W3 /WX)
endif()

add_executable(clock_test test.c)
target_link_libraries(clock_test PRIVATE timing)
