/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import React from 'react';

type EducationInfoTipProps = {children: React.ReactNode};

function EducationInfoTipInner(props: EducationInfoTipProps) {
  return (
    <Tooltip title={props.children}>
      <Icon icon="info" />
    </Tooltip>
  );
}

/** An "i" button with tooltip for education purpose */
export const EducationInfoTip = React.memo(EducationInfoTipInner);
