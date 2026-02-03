/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Barrel file for Jotai hooks.
 *
 * During the Recoil to Jotai migration, hooks are organized by conceptual group:
 * - useSplitDiffViewData: Hooks for diff view data loading
 * - (future) usePullRequest*: Hooks for pull request data
 * - (future) useGitHub*: Hooks for GitHub API interactions
 */

export {useSplitDiffViewData} from './useSplitDiffViewData';
export type {SplitDiffViewLoadableState} from './useSplitDiffViewData';
