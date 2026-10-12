/* SPDX-FileCopyrightText: 2026 Redrob Labs
 *
 * SPDX-License-Identifier: GPL-2.0-or-later */

#pragma once

/** \file
 * \ingroup bke
 *
 * The Redrob Shape product version, shown wherever the user reads a version: the splash, the
 * window title, the status bar and the About box. It must equal the repository's REDROB_VERSION
 * file (scripts/verify-patches.sh checks it).
 *
 * The engine version (BKE_blender_version_string, bpy.app.version) is NOT replaced: .blend files,
 * add-on compatibility and exporter metadata depend on it.
 */

#define REDROB_VERSION_STRING "0.1.0"
