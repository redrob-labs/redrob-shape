# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

execute_process(
  COMMAND
    "${CMAKE_COMMAND}" -P
    "${REDROB_AGENT_SOURCE_DIR}/tests/RejectUniversalMacOS.cmake"
  RESULT_VARIABLE result
  OUTPUT_VARIABLE output
  ERROR_VARIABLE error
)
set(combined "${output}\n${error}")
if(result EQUAL 0)
  message(FATAL_ERROR "Universal macOS target resolution unexpectedly succeeded")
endif()
if(NOT combined MATCHES "does not support universal macOS builds")
  message(FATAL_ERROR "Universal macOS failed for the wrong reason:\n${combined}")
endif()
