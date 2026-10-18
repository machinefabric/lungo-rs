# The C end-to-end test of the polyglot fixture: builds the generated package (POLYGLOT_PACKAGE)
# with every warning as an error, and a test program using its API.
cmake_minimum_required(VERSION 3.20)
project(polyglot_c_test C)

# `/`-separated: the path is also written into a C string below, where a Windows `\` would begin
# an escape sequence.
file(TO_CMAKE_PATH "${POLYGLOT_PACKAGE}" package)
add_subdirectory(${package} polyglot)
if(CMAKE_C_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(polyglot PRIVATE -Wall -Wextra -Werror)
elseif(MSVC)
  target_compile_options(polyglot PRIVATE /W3 /WX)
endif()

add_executable(polyglot_test test.c)
target_link_libraries(polyglot_test PRIVATE polyglot)
# The package's assurance document, which the program embeds byte for byte.
target_compile_definitions(polyglot_test PRIVATE POLYGLOT_ASSURANCE="${package}/assurance.json")
