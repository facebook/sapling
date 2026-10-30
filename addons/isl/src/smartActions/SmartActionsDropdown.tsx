/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button, buttonStyles} from 'isl-components/Button';
import {ButtonDropdown, styles} from 'isl-components/ButtonDropdown';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {getZoomLevel} from 'isl-components/zoom';
import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useEffect, useMemo, useRef, useState} from 'react';
import {cn} from 'shared/cn';
import {contextMenuState, useContextMenu} from 'shared/ContextMenu';
import {tracker} from '../analytics';
import serverAPI from '../ClientToServerAPI';
import {bulkFetchFeatureFlags} from '../featureFlags';
import {t} from '../i18n';
import {Internal} from '../Internal';
import platform from '../platform';
import {optimisticMergeConflicts} from '../previews';
import {repositoryInfo, useSupportsAgentPrompts} from '../serverAPIState';
import {showToast} from '../toast';
import type {CommitInfo, PlatformSpecificClientToServerMessages} from '../types';
import {bumpSmartAction, useSortedActions} from './smartActionsOrdering';
import type {ActionContext, ActionMenuItem, SmartActionConfig} from './types';
import {withUserContext} from './userContext';

const smartActionsConfig = [
  // Internal actions
  ...(Internal.smartActions?.smartActionsConfig ?? []),
  // Public actions
  // TODO: Add public actions here
] satisfies SmartActionConfig[];

// Additional feature flags needed by actions at runtime (not tied to a specific config's visibility)
const additionalFlagKeys: Array<string> = ['AICodeReviewAndFix'];

const smartActionFeatureFlagsAtom = atom<Promise<Record<string, boolean>>>(async () => {
  const flags: Record<string, boolean> = {};

  const flagNames: string[] = [];
  for (const config of smartActionsConfig) {
    if (config.featureFlag && Internal.featureFlags?.[config.featureFlag]) {
      flagNames.push(Internal.featureFlags[config.featureFlag]);
    }
  }

  // Also fetch additional flags needed by actions at runtime
  for (const key of additionalFlagKeys) {
    const flagName =
      Internal.featureFlags?.[key as keyof NonNullable<typeof Internal.featureFlags>];
    if (flagName && !flagNames.includes(flagName)) {
      flagNames.push(flagName);
    }
  }

  if (flagNames.length === 0) {
    return flags;
  }

  const results = await bulkFetchFeatureFlags(flagNames);

  // Map back from flag names to flag keys
  for (const config of smartActionsConfig) {
    if (config.featureFlag && Internal.featureFlags?.[config.featureFlag]) {
      const flagName = Internal.featureFlags[config.featureFlag];
      flags[config.featureFlag as string] = results[flagName] ?? false;
    }
  }
  // Map additional flags
  for (const key of additionalFlagKeys) {
    const flagName =
      Internal.featureFlags?.[key as keyof NonNullable<typeof Internal.featureFlags>];
    if (flagName) {
      flags[key] = results[flagName] ?? false;
    }
  }

  return flags;
});

const loadableFeatureFlagsAtom = loadable(smartActionFeatureFlagsAtom);

export function SmartActionsDropdown({commit}: {commit?: CommitInfo}) {
  const repo = useAtomValue(repositoryInfo);
  const conflicts = useAtomValue(optimisticMergeConflicts);
  const featureFlagsLoadable = useAtomValue(loadableFeatureFlagsAtom);
  const agentPrompts = useSupportsAgentPrompts();
  const dropdownButtonRef = useRef<HTMLButtonElement>(null);
  const isMenuOpen = useAtomValue(contextMenuState) != null;
  const wasMenuOpenOnPointerDown = useRef(false);

  const featureFlags = useMemo(
    () => (featureFlagsLoadable.state === 'hasData' ? featureFlagsLoadable.data : {}),
    [featureFlagsLoadable],
  );

  const context: ActionContext = useMemo(
    () => ({
      commit,
      repoPath: repo?.repoRoot,
      conflicts,
      featureFlags,
    }),
    [commit, repo?.repoRoot, conflicts, featureFlags],
  );

  const availableActionItems = useMemo(() => {
    const items: ActionMenuItem[] = [];

    if (featureFlagsLoadable.state === 'hasData') {
      for (const config of smartActionsConfig) {
        if (
          shouldShowSmartAction(
            config,
            context,
            config.featureFlag ? featureFlags[config.featureFlag as string] : true,
            agentPrompts,
          )
        ) {
          items.push({
            id: config.id,
            label: config.label,
            config,
          });
        }
      }
    }

    return items;
  }, [featureFlagsLoadable, context, featureFlags, agentPrompts]);

  const sortedActionItems = useSortedActions(availableActionItems);

  const [selectedAction, setSelectedAction] = useState<ActionMenuItem | undefined>(undefined);
  const contextTooltipToggle = useRef(new EventTarget());

  useEffect(() => {
    if (
      !selectedAction || // No action selected
      !sortedActionItems.find(item => item.id === selectedAction.id) // Selected action is no longer available
    ) {
      setSelectedAction(sortedActionItems[0]);
    }
  }, [selectedAction, sortedActionItems]);

  const openContextTooltip = useCallback(() => {
    contextTooltipToggle.current.dispatchEvent(new Event('change'));
  }, []);

  const contextMenu = useContextMenu(() =>
    sortedActionItems.map(actionItem => ({
      label: actionItem.label,
      onClick: (e?: MouseEvent) => {
        setSelectedAction(actionItem);
        bumpSmartAction(actionItem.id);
        if (e?.altKey) {
          // Defer to allow state update and re-render before toggling the tooltip
          setTimeout(openContextTooltip, 0);
        }
      },
      tooltip: (() => {
        const desc = resolveDescription(actionItem.config, context);
        return desc
          ? (Internal.smartActions?.renderModifierContextTooltip?.(desc) ?? t(desc))
          : undefined;
      })(),
    })),
  );

  if (featureFlagsLoadable.state !== 'hasData') {
    return null;
  }

  if (
    !Internal.smartActions?.showSmartActions ||
    sortedActionItems.length === 0 ||
    !selectedAction
  ) {
    return null;
  }

  let buttonComponent;

  const resolvedDesc = resolveDescription(selectedAction.config, context);
  const description = resolvedDesc ? t(resolvedDesc) : undefined;
  const tooltip = description
    ? (Internal.smartActions?.renderModifierContextTooltip?.(description) ?? description)
    : undefined;

  if (sortedActionItems.length === 1) {
    const singleAction = sortedActionItems[0];
    buttonComponent = (
      <SmartActionWithContext
        config={singleAction.config}
        context={context}
        tooltip={tooltip}
        additionalToggles={contextTooltipToggle.current}>
        <Button
          kind="icon"
          onClick={e => {
            if (e.altKey) {
              return;
            }
            e.stopPropagation();
            runSmartAction(singleAction.config, context);
            bumpSmartAction(singleAction.id);
          }}>
          <Icon icon="lightbulb-sparkle" />
          {singleAction.label}
        </Button>
      </SmartActionWithContext>
    );
  } else {
    buttonComponent = (
      <SmartActionWithContext
        config={selectedAction.config}
        context={context}
        tooltip={tooltip}
        additionalToggles={contextTooltipToggle.current}>
        <ButtonDropdown
          kind="icon"
          options={[]}
          selected={selectedAction}
          icon={<Icon icon="lightbulb-sparkle" />}
          onClick={(action, e) => {
            if (e.altKey) {
              return;
            }
            e.stopPropagation();
            runSmartAction(action.config, context);
            // Update the cache with the most recent action
            bumpSmartAction(action.id);
          }}
          onChangeSelected={() => {}}
          customSelectComponent={
            <Button
              className={cn(styles.select, buttonStyles.icon, styles.iconSelect)}
              onPointerDown={() => {
                wasMenuOpenOnPointerDown.current = isMenuOpen;
              }}
              onClick={e => {
                if (wasMenuOpenOnPointerDown.current) {
                  wasMenuOpenOnPointerDown.current = false;
                  e.stopPropagation();
                  return;
                }
                if (dropdownButtonRef.current) {
                  const rect = dropdownButtonRef.current.getBoundingClientRect();
                  const zoom = getZoomLevel();
                  const xOffset = 4 * zoom;
                  const centerX = rect.left + rect.width / 2 - xOffset;
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
      </SmartActionWithContext>
    );
  }

  return buttonComponent;
}

function SmartActionWithContext({
  config,
  context,
  tooltip,
  children,
  additionalToggles,
}: {
  config: SmartActionConfig;
  context: ActionContext;
  tooltip?: React.ReactNode;
  children: React.ReactNode;
  additionalToggles?: EventTarget;
}) {
  const ContextInput = Internal.smartActions?.ContextInput;

  if (!ContextInput) {
    if (tooltip) {
      return <Tooltip title={tooltip}>{children}</Tooltip>;
    }
    return <>{children}</>;
  }

  return (
    <Tooltip
      trigger="click"
      component={dismiss => (
        <ContextInput
          onSubmit={(userContext: string) => {
            runSmartAction(config, {...context, userContext});
            bumpSmartAction(config.id);
            dismiss();
          }}
          actionName={config.label}
        />
      )}
      title={tooltip}
      group="smart-action-context-input"
      additionalToggles={additionalToggles}>
      {children}
    </Tooltip>
  );
}

function shouldShowSmartAction(
  config: SmartActionConfig,
  context: ActionContext,
  passesFeatureFlag: boolean,
  supportsAgentPrompts: boolean,
): boolean {
  if (!passesFeatureFlag) {
    return false;
  }

  const usesAgentPrompt = supportsAgentPrompts && config.getAgentPrompt != null;
  if (
    !usesAgentPrompt &&
    config.platformRestriction &&
    !config.platformRestriction?.includes(platform.platformName)
  ) {
    return false;
  }

  return config.shouldShow?.(context) ?? true;
}

function resolveDescription(config: SmartActionConfig, context: ActionContext): string | undefined {
  if (config.description == null) {
    return undefined;
  }
  return typeof config.description === 'function'
    ? config.description(context)
    : config.description;
}

function runSmartAction(config: SmartActionConfig, context: ActionContext): void {
  tracker.track('SmartActionClicked', {
    extras: {action: config.trackEventName, withUserContext: context.userContext != null},
  });
  if (config.getAgentPrompt != null && platform.supportsAgentPrompts?.()) {
    const {displayPrompt, modelPrompt} = withUserContext(
      config.getAgentPrompt(context),
      context.userContext,
    );
    platform
      .promptAgent?.({
        actionId: config.id,
        displayPrompt,
        modelPrompt,
        awaitResponse: config.onAgentResponse != null,
      })
      .then(response => config.onAgentResponse?.(response ?? ''))
      .catch((error: Error) =>
        showToast(
          t('$action failed: $error', {replace: {$action: config.label, $error: error.message}}),
          {
            durationMs: 8000,
          },
        ),
      );
    return;
  }
  if (config.getMessagePayload) {
    const payload = config.getMessagePayload(context);
    serverAPI.postMessage({
      ...payload,
    } as PlatformSpecificClientToServerMessages);
  }
}
