# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.21)

find_program(REDROB_CARGO_EXECUTABLE cargo REQUIRED)
mark_as_advanced(REDROB_CARGO_EXECUTABLE)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
  project(redrob_shape_agent LANGUAGES NONE)
  include(CTest)
  add_test(
    NAME redrob_agent_rust_target_mapping
    COMMAND "${CMAKE_COMMAND}" -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/RustTargetChecks.cmake"
  )
  add_test(
    NAME redrob_agent_rejects_universal_macos
    COMMAND "${CMAKE_COMMAND}"
      -DREDROB_AGENT_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}
      -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/UniversalMacOSCheck.cmake"
  )
  add_test(
    NAME redrob_agent_multi_config_profile
    COMMAND "${CMAKE_COMMAND}"
      -DREDROB_AGENT_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}
      -DREDROB_AGENT_TEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}
      -DREDROB_CARGO_EXECUTABLE=${REDROB_CARGO_EXECUTABLE}
      -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/MultiConfigProfileCheck.cmake"
  )
endif()

include("${CMAKE_CURRENT_SOURCE_DIR}/cmake/RustTarget.cmake")
redrob_agent_resolve_rust_target(
  REDROB_AGENT_RUST_TARGET
  "${CMAKE_SYSTEM_NAME}"
  "${CMAKE_SYSTEM_PROCESSOR}"
  "${CMAKE_OSX_ARCHITECTURES}"
)
message(STATUS "Redrob agent Rust target: ${REDROB_AGENT_RUST_TARGET}")

set(REDROB_AGENT_CARGO_TARGET_DIR "${CMAKE_BINARY_DIR}/redrob_agent_cargo")
set(REDROB_AGENT_BINARY_NAME "redrob-shape-agent${CMAKE_EXECUTABLE_SUFFIX}")
set(REDROB_AGENT_TARGET_OUTPUT_DIR
  "${REDROB_AGENT_CARGO_TARGET_DIR}/${REDROB_AGENT_RUST_TARGET}"
)
set(REDROB_AGENT_DEBUG_BINARY
  "${REDROB_AGENT_TARGET_OUTPUT_DIR}/debug/${REDROB_AGENT_BINARY_NAME}"
)
set(REDROB_AGENT_RELEASE_BINARY
  "${REDROB_AGENT_TARGET_OUTPUT_DIR}/release/${REDROB_AGENT_BINARY_NAME}"
)

file(GLOB_RECURSE REDROB_AGENT_RUST_SOURCES CONFIGURE_DEPENDS
  "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs"
)

if(CMAKE_CONFIGURATION_TYPES)
  set(REDROB_AGENT_SELECTED_BINARY
    "$<IF:$<CONFIG:Debug>,${REDROB_AGENT_DEBUG_BINARY},${REDROB_AGENT_RELEASE_BINARY}>"
  )
else()
  if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(REDROB_AGENT_SELECTED_BINARY "${REDROB_AGENT_DEBUG_BINARY}")
  else()
    set(REDROB_AGENT_SELECTED_BINARY "${REDROB_AGENT_RELEASE_BINARY}")
  endif()
endif()

# A single configuration-selected Cargo invocation avoids making Debug builds
# depend on the release artifact with multi-config generators.
add_custom_target(redrob_shape_agent ALL
  COMMAND
    "${CMAKE_COMMAND}" -E env
    "CARGO_TARGET_DIR=${REDROB_AGENT_CARGO_TARGET_DIR}"
    "${REDROB_CARGO_EXECUTABLE}" build --locked
    $<$<NOT:$<CONFIG:Debug>>:--release>
    --target "${REDROB_AGENT_RUST_TARGET}"
    --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
  DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.lock"
    ${REDROB_AGENT_RUST_SOURCES}
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  COMMENT "Building Redrob Shape agent (${REDROB_AGENT_RUST_TARGET})"
  VERBATIM
  COMMAND_EXPAND_LISTS
)

set_property(TARGET redrob_shape_agent PROPERTY
  REDROB_AGENT_SELECTED_BINARY "${REDROB_AGENT_SELECTED_BINARY}"
)
set_property(TARGET redrob_shape_agent PROPERTY
  REDROB_AGENT_BINARY_NAME "${REDROB_AGENT_BINARY_NAME}"
)

function(redrob_agent_attach_to_target target_name)
  if(NOT TARGET "${target_name}")
    message(FATAL_ERROR "Cannot attach Redrob agent to unknown target: ${target_name}")
  endif()
  get_target_property(_selected_binary redrob_shape_agent REDROB_AGENT_SELECTED_BINARY)
  get_target_property(_binary_name redrob_shape_agent REDROB_AGENT_BINARY_NAME)
  add_dependencies("${target_name}" redrob_shape_agent)
  add_custom_command(TARGET "${target_name}" POST_BUILD
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
      "${_selected_binary}"
      "$<TARGET_FILE_DIR:${target_name}>/${_binary_name}"
    COMMENT "Copying Redrob Shape agent beside ${target_name}"
    VERBATIM
  )
endfunction()

if(NOT DEFINED REDROB_AGENT_INSTALL_DESTINATION)
  if(DEFINED CMAKE_INSTALL_BINDIR)
    set(REDROB_AGENT_INSTALL_DESTINATION "${CMAKE_INSTALL_BINDIR}")
  else()
    set(REDROB_AGENT_INSTALL_DESTINATION "bin")
  endif()
endif()
if(NOT DEFINED REDROB_AGENT_LICENSE_DESTINATION)
  set(REDROB_AGENT_LICENSE_DESTINATION "share/licenses/redrob-shape-agent")
endif()

install(
  PROGRAMS "${REDROB_AGENT_SELECTED_BINARY}"
  DESTINATION "${REDROB_AGENT_INSTALL_DESTINATION}"
  RENAME "${REDROB_AGENT_BINARY_NAME}"
)
install(
  FILES
    "${CMAKE_CURRENT_SOURCE_DIR}/THIRD_PARTY.md"
    "${CMAKE_CURRENT_SOURCE_DIR}/THIRD_PARTY-LICENSES.txt"
  DESTINATION "${REDROB_AGENT_LICENSE_DESTINATION}"
)
