/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';

const TEST_FEATURE_FLAG_NAME = 'isl_smart_actions_menu_test';
const onAgentResponse = jest.fn();

jest.mock('../Internal', () => ({
  Internal: {
    smartActions: {
      showSmartActions: true,
      smartActionsConfig: [
        {
          id: 'agent-action',
          label: 'Agent Action',
          trackEventName: 'TestAgentAction',
          platformRestriction: ['vscode'],
          getMessagePayload: () => ({type: 'platform/validateChangesWithAI'}),
          getAgentPrompt: () => ({displayPrompt: 'Do it', modelPrompt: 'Do the thing'}),
          onAgentResponse: (response: string) => onAgentResponse(response),
        },
        {
          id: 'vscode-action',
          label: 'VS Code Action',
          trackEventName: 'TestVSCodeAction',
          platformRestriction: ['vscode'],
          getMessagePayload: () => ({type: 'platform/createTestForModifiedCodeWithAI'}),
        },
      ],
    },
    featureFlags: {
      SmartActionsMenu: TEST_FEATURE_FLAG_NAME,
    },
  },
}));

jest.mock('../smartActions/smartActionsOrdering', () => ({
  bumpSmartAction: jest.fn(),
  useSortedActions: <T,>(items: T) => items,
}));

import {__TEST__ as featureFlagsTest} from '../featureFlags';
import {readAtom, writeAtom} from '../jotaiUtils';
import platform from '../platform';
import {reconnectingStatus} from '../serverAPIState';
import {SmartActionsDropdown} from '../smartActions/SmartActionsDropdown';
import {resetTestMessages} from '../testUtils';
import {toastQueueAtom} from '../toast';

describe('SmartActionsDropdown on agent prompt hosts', () => {
  const overriddenKeys = ['platformName', 'supportsAgentPrompts', 'promptAgent'] as const;
  let originals: Array<readonly [string, PropertyDescriptor | undefined]>;

  beforeEach(() => {
    originals = overriddenKeys.map(key => [key, Object.getOwnPropertyDescriptor(platform, key)]);
    resetTestMessages();
    onAgentResponse.mockReset();
    featureFlagsTest.overrideFeatureFlag(TEST_FEATURE_FLAG_NAME, true);
  });

  afterEach(() => {
    featureFlagsTest.clearFeatureFlagOverrides();
    for (const [key, descriptor] of originals) {
      if (descriptor == null) {
        delete (platform as unknown as Record<string, unknown>)[key];
      } else {
        Object.defineProperty(platform, key, descriptor);
      }
    }
  });

  async function renderDropdown() {
    act(() => {
      render(<SmartActionsDropdown />);
    });
    // Let the feature flags atom resolve.
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  function useAgentHost(promptAgent: (prompt: unknown) => Promise<string>) {
    Object.assign(platform, {
      platformName: 'vscode-agents',
      supportsAgentPrompts: () => true,
      promptAgent: jest.fn(promptAgent),
    });
  }

  it('shows only the actions that have an agent prompt', async () => {
    useAgentHost(() => Promise.resolve(''));
    await renderDropdown();
    expect(screen.getByText('Agent Action')).toBeInTheDocument();
    expect(screen.queryByText('VS Code Action')).not.toBeInTheDocument();
  });

  it.each([
    ['reports no support', () => false],
    ['is missing', undefined],
  ])('hides restricted actions when the capability check %s', async (_, check) => {
    Object.assign(platform, {platformName: 'vscode-agents', supportsAgentPrompts: check});
    await renderDropdown();
    expect(screen.queryByText('Agent Action')).not.toBeInTheDocument();
    expect(screen.queryByText('VS Code Action')).not.toBeInTheDocument();
  });

  it('updates when the host reports new capabilities after rendering', async () => {
    Object.assign(platform, {platformName: 'vscode-agents', supportsAgentPrompts: () => false});
    await renderDropdown();
    expect(screen.queryByText('Agent Action')).not.toBeInTheDocument();

    useAgentHost(() => Promise.resolve(''));
    act(() => writeAtom(reconnectingStatus, {type: 'open'}));
    expect(screen.getByText('Agent Action')).toBeInTheDocument();
  });

  it('sends the prompt and hands the response to the action', async () => {
    useAgentHost(() => Promise.resolve('all done'));
    await renderDropdown();
    await act(async () => {
      fireEvent.click(screen.getByText('Agent Action'));
      await Promise.resolve();
    });
    expect(platform.promptAgent).toHaveBeenCalledWith({
      actionId: 'agent-action',
      displayPrompt: 'Do it',
      modelPrompt: 'Do the thing',
      awaitResponse: true,
    });
    expect(onAgentResponse).toHaveBeenCalledWith('all done');
  });

  it.each([
    ['the agent fails', () => Promise.reject(new Error('agent unavailable')), 'agent unavailable'],
    ['the response is unusable', () => Promise.resolve('???'), 'bad response'],
  ])('tells the user when %s', async (_, promptAgent, error) => {
    onAgentResponse.mockImplementation(() => {
      throw new Error('bad response');
    });
    useAgentHost(promptAgent);
    await renderDropdown();
    await act(async () => {
      fireEvent.click(screen.getByText('Agent Action'));
      await new Promise(resolve => setTimeout(resolve, 0));
    });
    expect(String(readAtom(toastQueueAtom).last()?.message)).toContain(error);
  });
});
