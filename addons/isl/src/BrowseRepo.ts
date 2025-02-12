/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';
import type {Hash, RepoRelativePath} from './types';

import {revsetForComparison} from 'shared/Comparison';
import serverAPI from './ClientToServerAPI';
import {configBackedAtom} from './jotaiUtils';
import platform from './platform';
import {copyAndShowToast, showToast} from './toast';

export const supportsBrowseUrlForHash = configBackedAtom(
  'fbcodereview.code-browser-url',
  /* default */ false,
  /* readonly */ true,
  /* use raw value */ true,
);

export async function openBrowseUrlForHash(hash: Hash) {
  serverAPI.postMessage({type: 'getRepoUrlAtHash', revset: hash});
  const msg = await serverAPI.nextMessageMatching('gotRepoUrlAtHash', () => true);

  const url = msg.url;
  if (url.error) {
    showToast('Failed to get repo URL to browse', {durationMs: 5000});
    return;
  } else if (url.value == null) {
    return;
  }
  platform.openExternalLink(url.value);
}

export async function copyUrlForFile(path: RepoRelativePath, comparison: Comparison) {
  const revset = revsetForComparison(comparison);
  serverAPI.postMessage({type: 'getRepoUrlAtHash', revset, path});
  const msg = await serverAPI.nextMessageMatching('gotRepoUrlAtHash', () => true);

  const url = msg.url;
  if (url.error) {
    showToast('Failed to get repo URL to copy', {durationMs: 5000});
    return;
  } else if (url.value == null) {
    return;
  }
  copyAndShowToast(url.value, undefined);
}
