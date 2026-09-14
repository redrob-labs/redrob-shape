# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

function(redrob_agent_resolve_rust_target output_variable system_name system_processor osx_architectures)
  string(TOLOWER "${system_processor}" _processor)

  if(system_name STREQUAL "Windows")
    if(_processor MATCHES "^(amd64|x86_64)$")
      set(_target "x86_64-pc-windows-msvc")
    elseif(_processor MATCHES "^(arm64|aarch64)$")
      set(_target "aarch64-pc-windows-msvc")
    else()
      message(FATAL_ERROR "Redrob agent does not support Windows architecture: ${system_processor}")
    endif()
  elseif(system_name STREQUAL "Darwin")
    set(_architectures ${osx_architectures})
    list(LENGTH _architectures _architecture_count)
    if(_architecture_count GREATER 1)
      message(FATAL_ERROR
        "Redrob agent does not support universal macOS builds; configure exactly one of x86_64 or arm64"
      )
    elseif(_architecture_count EQUAL 1)
      list(GET _architectures 0 _mac_processor)
      string(TOLOWER "${_mac_processor}" _processor)
    endif()

    if(_processor STREQUAL "x86_64")
      set(_target "x86_64-apple-darwin")
    elseif(_processor MATCHES "^(arm64|aarch64)$")
      set(_target "aarch64-apple-darwin")
    else()
      message(FATAL_ERROR "Redrob agent does not support macOS architecture: ${_processor}")
    endif()
  elseif(system_name STREQUAL "Linux")
    if(_processor MATCHES "^(amd64|x86_64)$")
      set(_target "x86_64-unknown-linux-gnu")
    elseif(_processor MATCHES "^(arm64|aarch64)$")
      set(_target "aarch64-unknown-linux-gnu")
    else()
      message(FATAL_ERROR "Redrob agent does not support Linux architecture: ${system_processor}")
    endif()
  else()
    message(FATAL_ERROR "Redrob agent does not support target system: ${system_name}")
  endif()

  set(${output_variable} "${_target}" PARENT_SCOPE)
endfunction()
