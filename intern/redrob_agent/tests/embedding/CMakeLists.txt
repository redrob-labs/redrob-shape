# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.21)
project(redrob_agent_embedding_fixture LANGUAGES C)

add_executable(blender dummy.c)
add_subdirectory("${REDROB_AGENT_SOURCE_DIR}" redrob_agent)
redrob_agent_attach_to_target(blender)
