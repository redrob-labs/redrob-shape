# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

include("${CMAKE_CURRENT_LIST_DIR}/../cmake/RustTarget.cmake")

function(assert_target expected system processor architectures)
  redrob_agent_resolve_rust_target(actual "${system}" "${processor}" "${architectures}")
  if(NOT actual STREQUAL expected)
    message(FATAL_ERROR
      "Expected ${expected} for ${system}/${processor}/${architectures}, got ${actual}"
    )
  endif()
endfunction()

assert_target("x86_64-pc-windows-msvc" "Windows" "AMD64" "")
assert_target("aarch64-pc-windows-msvc" "Windows" "ARM64" "")
assert_target("x86_64-unknown-linux-gnu" "Linux" "x86_64" "")
assert_target("aarch64-unknown-linux-gnu" "Linux" "aarch64" "")
assert_target("x86_64-apple-darwin" "Darwin" "x86_64" "x86_64")
assert_target("aarch64-apple-darwin" "Darwin" "arm64" "arm64")
