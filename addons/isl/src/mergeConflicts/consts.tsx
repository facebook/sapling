/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {t} from '../i18n';

export const CONFLICT_SIDE_LABELS = {
  /* Label used for the local / destination side of a conflict */
  local: t('dest - rebasing onto'),
  /* Shortened label used for the local / destination side of a conflict, when there's not good space for the full label */
  localShort: t('dest'),
  /* Label used for the incoming / source side of a conflict */
  incoming: t('source - being rebased'),
  /* Shortened label used for the incoming / source side of a conflict, when there's not good space for the full label */
  incomingShort: t('source'),
};
