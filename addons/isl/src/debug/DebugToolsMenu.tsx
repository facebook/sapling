/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Heartbeat} from '../heartbeat';
import type {ReactNode} from 'react';
import type {ExclusiveOr} from 'shared/typeUtils';

import {holdingAltAtom} from '../ChangedFile';
import {debugLogMessageTraffic} from '../ClientToServerAPI';
import {FlexRow} from '../ComponentUtils';
import {DropdownField, DropdownFields} from '../DropdownFields';
import {InlineErrorBadge} from '../ErrorNotice';
import {Subtle} from '../Subtle';
import {Tooltip} from '../Tooltip';
import {DagCommitInfo} from '../dag/dagCommitInfo';
import {useHeartbeat} from '../heartbeat';
import {t, T} from '../i18n';
import {atomWithOnChange, localStorageBackedAtom, readAtom} from '../jotaiUtils';
import platform from '../platform';
import {dagWithPreviews} from '../previews';
import {RelativeDate} from '../relativeDate';
import {
  latestCommitsData,
  latestUncommittedChangesData,
  mergeConflicts,
  repositoryInfo,
} from '../serverAPIState';
import {useShowToast} from '../toast';
import {isDev} from '../utils';
import {ComponentExplorerButton} from './ComponentExplorer';
import {readInterestingAtoms, serializeAtomsState} from './getInterestingAtoms';
import {VSCodeBadge, VSCodeButton, VSCodeCheckbox} from '@vscode/webview-ui-toolkit/react';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useState, useCallback, useEffect} from 'react';

import './DebugToolsMenu.css';

/* eslint-disable no-console */

export default function DebugToolsMenu({dismiss}: {dismiss: () => unknown}) {
  return (
    <DropdownFields
      title={<T>Internal Debugging Tools</T>}
      icon="pulse"
      data-testid="internal-debug-tools-dropdown"
      className="internal-debug-tools-dropdown">
      <Subtle>
        <T>
          This data is only intended for debugging Interactive Smartlog and may not capture all
          issues.
        </T>
      </Subtle>
      <DropdownField title={<T>Performance</T>}>
        <DebugPerfInfo />
      </DropdownField>
      <DropdownField title={<T>Commit graph</T>}>
        <DebugDagInfo />
      </DropdownField>
      <DropdownField title={<T>Internal State</T>}>
        <InternalState />
      </DropdownField>
      <DropdownField title={<T>Server/Client Messages</T>}>
        <ServerClientMessageLogging />
      </DropdownField>
      <DropdownField title={<T>Component Explorer</T>}>
        <ComponentExplorerButton dismiss={dismiss} />
      </DropdownField>
    </DropdownFields>
  );
}

export const enableReduxTools = localStorageBackedAtom<boolean>('isl.debug-redux-tools', false);

function InternalState() {
  const [reduxTools, setReduxTools] = useAtom(enableReduxTools);
  const showToast = useShowToast();
  const generate = () => {
    // No need for useAtomValue - no need to re-render or recalculate this function.
    const needSerialize = readAtom(holdingAltAtom);
    const atomsState = readInterestingAtoms();
    const value = needSerialize ? serializeAtomsState(atomsState) : atomsState;
    console.log('jotai state:', value);
    showToast.show(`logged jotai state to console!${needSerialize ? ' (serialized)' : ''}`);
  };

  return (
    <div>
      <FlexRow>
        <Tooltip
          placement="bottom"
          title={t(
            'Capture a snapshot of selected Jotai atom states, log it to the dev tools console.',
          )}>
          <VSCodeButton onClick={generate} appearance="secondary">
            <T>Take Snapshot</T>
          </VSCodeButton>
        </Tooltip>
        <Tooltip
          placement="bottom"
          title={t(
            'Log persisted state (localStorage or vscode storage) to the dev tools console.',
          )}>
          <VSCodeButton
            onClick={() => {
              console.log('persisted state:', platform.getAllTemporaryState());
              showToast.show('logged persisted state to console!');
            }}
            appearance="secondary">
            <T>Log Persisted State</T>
          </VSCodeButton>
        </Tooltip>
        <Tooltip
          placement="bottom"
          title={t(
            'Clear any persisted state (localStorage or vscode storage). Usually only matters after restarting.',
          )}>
          <VSCodeButton
            onClick={() => {
              platform.clearTemporaryState();
              console.log('--- cleared isl persisted state ---');
              showToast.show('cleared persisted state');
            }}
            appearance="secondary">
            <T>Clear Persisted State</T>
          </VSCodeButton>
        </Tooltip>
      </FlexRow>
      {isDev && (
        <VSCodeCheckbox checked={reduxTools} onChange={() => setReduxTools(v => !v)}>
          Integrate with Redux DevTools
        </VSCodeCheckbox>
      )}
    </div>
  );
}

const logMessagesState = atomWithOnChange(atom(debugLogMessageTraffic.shoudlLog), newValue => {
  debugLogMessageTraffic.shoudlLog = newValue;
  console.log(`----- ${newValue ? 'Enabled' : 'Disabled'} Logging Messages -----`);
});

function ServerClientMessageLogging() {
  const [shouldLog, setShouldLog] = useAtom(logMessagesState);
  return (
    <div>
      <VSCodeCheckbox
        checked={shouldLog}
        onChange={e => setShouldLog((e.target as HTMLInputElement).checked)}>
        <T>Log messages</T>
      </VSCodeCheckbox>
    </div>
  );
}

function DebugPerfInfo() {
  const latestStatus = useAtomValue(latestUncommittedChangesData);
  const latestLog = useAtomValue(latestCommitsData);
  const latestConflicts = useAtomValue(mergeConflicts);
  const heartbeat = useHeartbeat();
  const repoInfo = useAtomValue(repositoryInfo);
  let commandName = 'sl';
  if (repoInfo?.type === 'success') {
    commandName = repoInfo.command;
  }
  return (
    <div>
      {heartbeat.type === 'timeout' ? (
        <InlineErrorBadge error={new Error(t('Heartbeat timeout'))}>
          <T>Heartbeat timed out</T>
        </InlineErrorBadge>
      ) : (
        <FetchDurationInfo
          name={<T>ISL Server Ping</T>}
          duration={(heartbeat as Heartbeat & {type: 'success'})?.rtt}
        />
      )}
      <FetchDurationInfo
        name={<T replace={{$cmd: commandName}}>$cmd status</T>}
        start={latestStatus.fetchStartTimestamp}
        end={latestStatus.fetchCompletedTimestamp}
      />
      <FetchDurationInfo
        name={<T replace={{$cmd: commandName}}>$cmd log</T>}
        start={latestLog.fetchStartTimestamp}
        end={latestLog.fetchCompletedTimestamp}
      />
      <FetchDurationInfo
        name={<T>Merge Conflicts</T>}
        start={latestConflicts?.fetchStartTimestamp}
        end={latestConflicts?.fetchCompletedTimestamp}
      />
    </div>
  );
}

function FetchDurationInfo(
  props: {name: ReactNode} & ExclusiveOr<{start?: number; end?: number}, {duration: number}>,
) {
  const {name} = props;
  const {end, start, duration} = props;
  const deltaMs = duration != null ? duration : end == null || start == null ? null : end - start;
  const assessment =
    deltaMs == null ? 'none' : deltaMs < 1000 ? 'fast' : deltaMs < 3000 ? 'ok' : 'slow';
  return (
    <div className={`fetch-duration-info fetch-duration-${assessment}`}>
      {name} <VSCodeBadge>{deltaMs == null ? 'N/A' : `${deltaMs}ms`}</VSCodeBadge>
      {end == null ? null : (
        <Subtle>
          <Tooltip title={new Date(end).toLocaleString()} placement="right">
            <RelativeDate date={end} />
          </Tooltip>
        </Subtle>
      )}
    </div>
  );
}

function useMeasureDuration(slowOperation: () => void): number | null {
  const [measured, setMeasured] = useState<null | number>(null);
  useEffect(() => {
    requestIdleCallback(() => {
      const startTime = performance.now();
      slowOperation();
      const endTime = performance.now();
      setMeasured(endTime - startTime);
    });
    return () => setMeasured(null);
  }, [slowOperation]);
  return measured;
}

function DebugDagInfo() {
  const dag = useAtomValue(dagWithPreviews);
  const dagRenderBenchmark = useCallback(() => {
    // Slightly change the dag to invalidate its caches.
    const noise = performance.now();
    const newDag = dag.add([DagCommitInfo.fromCommitInfo({hash: `dummy-${noise}`, parents: []})]);
    newDag.renderToRows(newDag.subsetForRendering());
  }, [dag]);

  const dagSize = dag.all().size;
  const dagDisplayedSize = dag.subsetForRendering().size;
  const dagSortMs = useMeasureDuration(dagRenderBenchmark);

  return (
    <div>
      <T>Size: </T>
      {dagSize}
      <br />
      <T>Displayed: </T>
      {dagDisplayedSize}
      <br />
      <>
        <T>Render calculation: </T>
        {dagSortMs == null ? (
          <T>(Measuring)</T>
        ) : (
          <>
            {dagSortMs.toFixed(1)} <T>ms</T>
          </>
        )}
        <br />
      </>
    </div>
  );
}
