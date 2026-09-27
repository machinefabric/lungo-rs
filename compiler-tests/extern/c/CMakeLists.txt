# The host of the C packages: `provider` as a package of its own, `consumer` embedded, and the
# runtime found here so both see it.
cmake_minimum_required(VERSION 3.20)
project(extern_c_test C)

find_package(lungo ${LUNGO_VERSION} EXACT CONFIG REQUIRED PATHS ${LUNGO_RUNTIME} NO_DEFAULT_PATH)
add_subdirectory(${PROVIDER_PACKAGE} provider)
add_subdirectory(${CONSUMER_PACKAGE} consumer)
# The consumer's layout checks read the provider's header.
target_link_libraries(consumer PUBLIC provider)

add_executable(extern_test test.c)
target_link_libraries(extern_test PRIVATE consumer provider)
