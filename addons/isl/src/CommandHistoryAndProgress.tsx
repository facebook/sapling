/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';
import type {ValidatedRepoInfo} from './types';

import {Icon} from './Icon';
import {Tooltip} from './Tooltip';
import {t} from './i18n';
import {operationList, queuedOperations, repositoryInfo} from './serverAPIState';
import {CommandRunner} from './types';
import {useRecoilValue} from 'recoil';

import './CommandHistoryAndProgress.css';

function displayOperationArgs(info: ValidatedRepoInfo, operation: Operation) {
  const commandName =
    operation.runner === CommandRunner.Sapling
      ? /[^\\/]+$/.exec(info.command)?.[0] ?? 'sl'
      : // TODO: we currently don't know the command name when it's not sapling
        '';
  return (
    commandName +
    ' ' +
    operation
      .getArgs()
      .map(arg => {
        if (typeof arg === 'object') {
          switch (arg.type) {
            case 'repo-relative-file':
              return arg.path;
            case 'succeedable-revset':
              return arg.revset;
          }
        }
        return arg;
      })
      .join(' ')
  );
}

export function CommandHistoryAndProgress() {
  const list = useRecoilValue(operationList);
  const queued = useRecoilValue(queuedOperations);

  const info = useRecoilValue(repositoryInfo);
  if (info?.type !== 'success') {
    return null;
  }

  const progress = list.currentOperation;
  if (progress == null) {
    return null;
  }

  const commandForDisplay = displayOperationArgs(info, progress.operation);
  const command = <code className="progress-container-command">{commandForDisplay}</code>;

  let label;
  let icon;
  let showLastLineOfOutput = false;
  if (progress.exitCode == null) {
    label = <span>Running {command}</span>;
    icon = <Icon icon="loading" />;
    showLastLineOfOutput = true;
  } else if (progress.exitCode === 0) {
    label = <span>{command}</span>;
    icon = <Icon icon="pass" aria-label={t('Command exited successfully')} />;
  } else {
    label = <span>{command}</span>;
    icon = <Icon icon="error" aria-label={t('Command exited unsuccessfully')} />;
    showLastLineOfOutput = true;
  }

  return (
    <div className="progress-container" data-testid="progress-container">
      <Tooltip
        component={() => (
          <div className="progress-command-tooltip">
            <div className="progress-command-tooltip-command">
              <strong>Command: </strong>
              {commandForDisplay}
            </div>
            <br />
            <b>Command output:</b>
            <br />
            <pre>{progress.commandOutput?.join('') || 'No output'}</pre>
          </div>
        )}>
        {queued.length > 0 ? (
          <div className="queued-operations-container" data-testid="queued-commands">
            <strong>Next to run</strong>
            {queued.map(op => (
              <div key={op.id} id={op.id} className="queued-operation">
                <code>{displayOperationArgs(info, op)}</code>
              </div>
            ))}
          </div>
        ) : null}

        <div className="progress-container-row">
          {icon}
          {label}
        </div>
        {showLastLineOfOutput ? (
          <div className="progress-container-row">
            <div>
              {progress.commandOutput?.slice(-1).map((line, i) => (
                <code key={i}>{line}</code>
              ))}
            </div>
          </div>
        ) : null}
      </Tooltip>
    </div>
  );
}
