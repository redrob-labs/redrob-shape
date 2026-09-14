# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

include("${CMAKE_CURRENT_LIST_DIR}/../cmake/RustTarget.cmake")
redrob_agent_resolve_rust_target(actual "Darwin" "arm64" "arm64;x86_64")
message(FATAL_ERROR "Universal macOS architecture unexpectedly resolved to ${actual}")
