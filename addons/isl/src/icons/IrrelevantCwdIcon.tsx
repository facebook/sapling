/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {IconStack} from './IconStack';

export const IrrelevantCwdIcon = () => (
  <IconStack>
    <Icon icon="folder" />
    <Icon icon="chrome-close" />
  </IconStack>
);
