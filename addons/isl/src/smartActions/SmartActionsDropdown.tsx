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
import {getZoomLevel} from 'isl-components/zoom';
import {useAtomValue} from 'jotai';
import {useRef, useState} from 'react';
import {useContextMenu} from 'shared/ContextMenu';
import {tracker} from '../analytics';
import serverAPI from '../ClientToServerAPI';
import {useFeatureFlagAsync, useFeatureFlagSync} from '../featureFlags';
import {Internal} from '../Internal';
import platform from '../platform';
import {optimisticMergeConflicts} from '../previews';
import {repositoryInfo} from '../serverAPIState';
import type {CommitInfo, PlatformSpecificClientToServerMessages} from '../types';
import {smartActionsConfig} from './actionConfigs';
import type {ActionContext, SmartActionConfig} from './types';

type ActionMenuItem = {
  id: string;
  label: string;
  config: SmartActionConfig;
};

export function SmartActionsDropdown({commit}: {commit?: CommitInfo}) {
  const smartActionsMenuEnabled = useFeatureFlagSync(Internal.featureFlags?.SmartActionsMenu);
  const repo = useAtomValue(repositoryInfo);
  const conflicts = useAtomValue(optimisticMergeConflicts);

  const context: ActionContext = {
    commit,
    repoPath: repo?.repoRoot,
    conflicts,
  };

  // Load all feature flags
  const featureFlagResults: Record<string, boolean> = {};
  for (const config of smartActionsConfig) {
    if (config.featureFlag) {
      // Smart actions are constant and have a fixed order,
      // so it's safe to use the hook in a loop here
      // eslint-disable-next-line react-hooks/rules-of-hooks
      featureFlagResults[config.featureFlag as string] = useFeatureFlagAsync(
        Internal.featureFlags?.[config.featureFlag],
      );
    }
  }

  const availableActionItems: ActionMenuItem[] = [];
  for (const config of smartActionsConfig) {
    if (
      shouldShowSmartAction(
        config,
        context,
        config.featureFlag ? featureFlagResults[config.featureFlag as string] : true,
      )
    ) {
      availableActionItems.push({
        id: config.id,
        label: config.label,
        config,
      });
    }
  }

  const [selectedAction, setSelectedAction] = useState<ActionMenuItem | undefined>(
    availableActionItems[0],
  );

  const contextMenu = useContextMenu(() =>
    availableActionItems.map(actionItem => ({
      label: (
        // Mark the current action as selected
        <Row>
          <Icon icon={actionItem.id === selectedAction?.id ? 'check' : 'blank'} />
          {actionItem.label}
        </Row>
      ),
      onClick: () => {
        setSelectedAction(actionItem);
      },
    })),
  );
  const dropdownButtonRef = useRef<HTMLButtonElement>(null);

  if (
    !smartActionsMenuEnabled ||
    !Internal.showSmartActions ||
    availableActionItems.length === 0 ||
    !selectedAction
  ) {
    return null;
  }

  if (availableActionItems.length === 1) {
    return (
      <Button kind="icon" onClick={() => runSmartAction(availableActionItems[0].config, context)}>
        <Icon icon="lightbulb-sparkle" />
        {availableActionItems[0].label}
      </Button>
    );
  }

  return (
    <ButtonDropdown
      kind="icon"
      options={[]}
      selected={selectedAction}
      icon={<Icon icon="lightbulb-sparkle" />}
      onClick={action => runSmartAction(action.config, context)}
      onChangeSelected={() => {}}
      customSelectComponent={
        <Button
          {...stylex.props(styles.select, buttonStyles.icon, styles.iconSelect)}
          onClick={e => {
            if (dropdownButtonRef.current) {
              const rect = dropdownButtonRef.current.getBoundingClientRect();
              const zoom = getZoomLevel();
              const xOffset = 4 * zoom;
              const centerX = rect.left + rect.width / 2 - xOffset;
              // Position arrow at the top or bottom edge of button depending on which half of screen we're in
              const isTopHalf =
                (rect.top + rect.height / 2) / zoom <= window.innerHeight / zoom / 2;
              const yOffset = 5 * zoom;
              const edgeY = isTopHalf ? rect.bottom - yOffset : rect.top + yOffset;
              Object.defineProperty(e, 'clientX', {value: centerX, configurable: true});
              Object.defineProperty(e, 'clientY', {value: edgeY, configurable: true});
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

function shouldShowSmartAction(
  config: SmartActionConfig,
  context: ActionContext,
  passesFeatureFlag: boolean,
): boolean {
  if (!passesFeatureFlag) {
    return false;
  }

  if (config.platformRestriction && !config.platformRestriction?.includes(platform.platformName)) {
    return false;
  }

  return config.shouldShow?.(context) ?? true;
}

function runSmartAction(config: SmartActionConfig, context: ActionContext): void {
  tracker.track('SmartActionClicked', {extras: {action: config.trackEventName}});
  if (config.getMessagePayload) {
    const payload = config.getMessagePayload(context);
    serverAPI.postMessage({
      ...payload,
    } as PlatformSpecificClientToServerMessages);
  }
}
