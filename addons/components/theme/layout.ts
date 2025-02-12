/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {spacing} from './tokens.stylex';

export const layout = stylex.create({
  flexRow: {
    display: 'flex',
    flexDirection: 'row',
    gap: spacing.pad,
    alignItems: 'center',
  },
  flexCol: {
    display: 'flex',
    flexDirection: 'column',
    gap: spacing.pad,
    alignItems: 'center',
  },
  fullWidth: {
    width: '100%',
  },
  padding: {
    padding: spacing.pad,
  },
  paddingInline: {
    paddingInline: spacing.pad,
  },
  paddingBlock: {
    paddingBlock: spacing.pad,
  },
  margin: {
    margin: spacing.pad,
  },
  marginInline: {
    marginInline: spacing.pad,
  },
  marginBlock: {
    marginBlock: spacing.pad,
  },
});
