/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DagCommitInfo} from './dag';

import {t} from '../i18n';

export const YOU_ARE_HERE_VIRTUAL_COMMIT: DagCommitInfo = {
  hash: 'YOU_ARE_HERE',
  title: '',
  parents: [],
  phase: 'draft',
  isHead: false,
  date: new Date(8640000000000000),
  bookmarks: [],
  remoteBookmarks: [],
  author: '',
  description: t('You are here'),
  filesSample: [],
  totalFileCount: 0,
  isYouAreHere: true,
};
