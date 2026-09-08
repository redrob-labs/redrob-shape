# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

set(binary_dir "${REDROB_AGENT_TEST_BINARY_DIR}/multi-config-profile")
file(REMOVE_RECURSE "${binary_dir}")
execute_process(
  COMMAND
    "${CMAKE_COMMAND}" -G "Ninja Multi-Config"
    -DREDROB_CARGO_EXECUTABLE=${REDROB_CARGO_EXECUTABLE}
    -DREDROB_AGENT_SOURCE_DIR=${REDROB_AGENT_SOURCE_DIR}
    -S "${REDROB_AGENT_SOURCE_DIR}/tests/embedding"
    -B "${binary_dir}"
  RESULT_VARIABLE configure_result
  OUTPUT_VARIABLE configure_output
  ERROR_VARIABLE configure_error
)
if(NOT configure_result EQUAL 0)
  message(FATAL_ERROR "Multi-config fixture failed to configure:\n${configure_output}\n${configure_error}")
endif()

file(READ "${binary_dir}/CMakeFiles/impl-Debug.ninja" debug_rules)
file(READ "${binary_dir}/CMakeFiles/impl-Release.ninja" release_rules)
if(NOT debug_rules MATCHES "cargo[^\n]*build --locked")
  message(FATAL_ERROR "Debug rules do not contain the Cargo build command")
endif()
if(debug_rules MATCHES "cargo[^\n]*build --locked[^\n]*--release")
  message(FATAL_ERROR "Debug rules unexpectedly select Cargo's release profile")
endif()
if(NOT release_rules MATCHES "cargo[^\n]*build --locked[^\n]*--release")
  message(FATAL_ERROR "Release rules do not select Cargo's release profile")
endif()
if(NOT debug_rules MATCHES "redrob-reblend-agent")
  message(FATAL_ERROR "Debug rules do not copy the sidecar beside the embedded executable")
endif()
if(NOT debug_rules MATCHES "blender[^\n]*redrob_reblend_agent")
  message(FATAL_ERROR "The embedded blender target does not depend on the sidecar target")
endif()
