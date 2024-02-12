/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {SystemStyleObject} from '@styled-system/css';

import {Button} from '@primer/react';
import React from 'react';

const DEFAULT_STYLES = {
  backgroundColor: 'canvas.subtle',
  color: 'fg.default',
  fontWeight: 'normal',
  ':hover': {
    fontWeight: 'bold',
  },
  ':disabled': {
    color: 'fg.subtle',
    cursor: 'not-allowed',
    fontWeight: 'normal',
  },
};

const SELECTED_STYLES = {
  backgroundColor: 'accent.fg',
  color: 'fg.onEmphasis',
  fontWeight: 'bold',
};

function getStyles(isSelected: boolean): SystemStyleObject {
  return isSelected ? SELECTED_STYLES : DEFAULT_STYLES;
}

type Props = {
  label: string;
  isSelected: boolean;
  onToggle: (isSelected: boolean) => void;
  isDisabled?: boolean;
  width?: number;
};

function ToggleButton({
  label,
  isSelected,
  onToggle,
  isDisabled = false,
  width,
}: Props): React.ReactElement {
  return (
    <Button
      disabled={isDisabled}
      variant="outline"
      onClick={() => onToggle(!isSelected)}
      sx={{...getStyles(isSelected), width}}>
      {label}
    </Button>
  );
}

export default React.memo(ToggleButton);
