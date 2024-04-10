/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FieldConfig} from './types';

export const OSSCommitMessageFieldSchema: Array<FieldConfig> = [
  {key: 'Title', type: 'title', icon: 'milestone'},
  {key: 'Description', type: 'textarea', icon: 'note'},
];
