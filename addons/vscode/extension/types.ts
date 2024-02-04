/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Not all features of the VS Code API may be enabled / rolled out, so they are controlled individually.
 * In OSS, they are all enabled. Interally, they may be disabled while transitioning from an older system.
 * blame => inline and toggleable blame
 * sidebar => VS Code SCM API, VS Code Source Control sidebar entry.
 * diffview => diff commands, gutters. Requires 'sidebar'.
 * */
export type EnabledSCMApiFeature = 'blame' | 'sidebar';
