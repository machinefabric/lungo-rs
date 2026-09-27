# The C end-to-end test of the polyglot fixture: builds the generated package (POLYGLOT_PACKAGE)
# with every warning as an error, and a test program using its API.
cmake_minimum_required(VERSION 3.20)
project(polyglot_c_test C)

add_subdirectory(${POLYGLOT_PACKAGE} polyglot)
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(polyglot PRIVATE -Wall -Wextra -Werror)
elseif(MSVC)
  target_compile_options(polyglot PRIVATE /W3 /WX)
endif()

add_executable(polyglot_test test.c)
target_link_libraries(polyglot_test PRIVATE polyglot)
