/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {T} from './i18n';

import {DropdownFields} from './DropdownFields';
import './SmartActionsMenu.css';

export function SmartActionsMenu() {
  return (
    <Tooltip
      component={dismiss => <SmartActions dismiss={dismiss} />}
      trigger="click"
      title={<T>Smart Actions...</T>}>
      <Button icon data-testid="smart-actions-button" className="smart-actions-button">
        <Icon icon="lightbulb" />
      </Button>
    </Tooltip>
  );
}

function SmartActions({dismiss}: {dismiss: () => void}) {
  return (
    <DropdownFields
      title={<T>Smart Actions</T>}
      icon="lightbulb"
      className="smart-actions-dropdown"
      data-testid="smart-actions-dropdown">
      <Button
        onClick={e => {
          dismiss();
          e.stopPropagation();
        }}>
        Hello!
      </Button>
    </DropdownFields>
  );
}
