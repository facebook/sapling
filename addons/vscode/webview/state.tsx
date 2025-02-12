/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {readAtom, writeAtom} from 'isl/src/jotaiUtils';
import {registerDisposable} from 'isl/src/utils';
import {atom} from 'jotai';
import serverAPI from '../../isl/src/ClientToServerAPI';

/** Should match the sapling.comparisonPanelMode enum in package.json */
export enum ComparisonPanelMode {
  Auto = 'Auto',
  AlwaysOpenPanel = 'Always Separate Panel',
}

export const comparisonPanelMode = atom<undefined | ComparisonPanelMode>(ComparisonPanelMode.Auto);
serverAPI.postMessage({
  type: 'platform/subscribeToVSCodeConfig',
  config: 'sapling.comparisonPanelMode',
});
registerDisposable(
  comparisonPanelMode,
  serverAPI.onMessageOfType('platform/vscodeConfigChanged', config => {
    if (config.config === 'sapling.comparisonPanelMode' && typeof config.value === 'string') {
      writeAtom(comparisonPanelMode, config.value as ComparisonPanelMode);
    }
  }),
  import.meta.hot,
);

export const setComparisonPanelMode = (mode: ComparisonPanelMode) => {
  // Optimistically set the state locally, and later get rewritten to the same value by the server.
  // NOTE: This relies on the server responding to these events in-order to ensure eventual consistency.
  writeAtom(comparisonPanelMode, mode);
  serverAPI.postMessage({
    type: 'platform/setVSCodeConfig',
    config: 'sapling.comparisonPanelMode',
    value: mode,
    scope: 'global',
  });
};

export const getComparisonPanelMode = () => {
  return readAtom(comparisonPanelMode) ?? 'Auto';
};
