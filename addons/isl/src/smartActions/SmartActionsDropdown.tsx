/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button, buttonStyles} from 'isl-components/Button';
import {ButtonDropdown, styles} from 'isl-components/ButtonDropdown';
import {Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {useRef} from 'react';
import {useContextMenu} from 'shared/ContextMenu';
import {useFeatureFlagSync} from '../featureFlags';
import {Internal} from '../Internal';

export function SmartActionsDropdown() {
  const smartActionsMenuEnabled = useFeatureFlagSync(Internal.featureFlags?.SmartActionsMenu);
  const contextMenu = useContextMenu(() => {
    return [
      {
        label: (
          <Row>
            <Icon icon="check" />
            Primary action
          </Row>
        ),
      },
      {
        label: (
          <Row>
            <Icon icon="blank" />
            Other action
          </Row>
        ),
      },
    ];
  });
  const dropdownButtonRef = useRef<HTMLButtonElement>(null);

  if (!smartActionsMenuEnabled) {
    return null;
  }

  return (
    <ButtonDropdown
      kind="icon"
      options={[]}
      selected={{id: 'primary-action', label: 'Primary action'}}
      icon={<Icon icon="lightbulb-sparkle" />}
      onClick={() => {}}
      onChangeSelected={() => {}}
      customSelectComponent={
        <Button
          {...stylex.props(styles.select, buttonStyles.icon, styles.iconSelect)}
          onClick={e => {
            if (dropdownButtonRef.current) {
              const rect = dropdownButtonRef.current.getBoundingClientRect();
              // Create a synthetic event with clientX/clientY in the center of the button
              const centerX = rect.left + rect.width / 2;
              const centerY = rect.top + rect.height / 2;
              Object.defineProperty(e, 'clientX', {value: centerX, configurable: true});
              Object.defineProperty(e, 'clientY', {value: centerY, configurable: true});
            }
            contextMenu(e);
            e.stopPropagation();
          }}
          ref={dropdownButtonRef}
        />
      }
    />
  );
}
