/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';
import type {ValidatedRepoInfo} from './types';
import type {ReactNode} from 'react';

import {Delayed} from './Delayed';
import {Tooltip} from './Tooltip';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {
  operationList,
  queuedOperations,
  repositoryInfo,
  useAbortRunningOperation,
} from './serverAPIState';
import {CommandRunner} from './types';
import {short} from './utils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import './CommandHistoryAndProgress.css';
import {notEmpty, truncate} from 'shared/utils';

function OperationDescription(props: {
  info: ValidatedRepoInfo;
  operation: Operation;
  className?: string;
  long?: boolean;
}): React.ReactElement {
  const {info, operation, className} = props;
  const desc = operation.getDescriptionForDisplay();

  const reviewProvider = useRecoilValue(codeReviewProvider);

  if (desc?.description) {
    return <span className={className}>{desc.description}</span>;
  }

  const commandName =
    operation.runner === CommandRunner.Sapling
      ? /[^\\/]+$/.exec(info.command)?.[0] ?? 'sl'
      : operation.runner === CommandRunner.CodeReviewProvider
      ? reviewProvider?.cliName
      : operation.runner === CommandRunner.InternalArcanist
      ? CommandRunner.InternalArcanist
      : null;
  return (
    <code className={className}>
      {(commandName ?? '') +
        ' ' +
        operation
          .getArgs()
          .map(arg => {
            if (typeof arg === 'object') {
              switch (arg.type) {
                case 'config':
                  // don't show configs in the UI
                  return undefined;
                case 'repo-relative-file':
                  return arg.path;
                case 'exact-revset':
                case 'succeedable-revset':
                  return props.long
                    ? arg.revset
                    : // truncate full commit hashes to short representation visually
                    // revset could also be a remote bookmark, so only do this if it looks like a hash
                    /[a-z0-9]{40}/.test(arg.revset)
                    ? short(arg.revset)
                    : arg.revset;
              }
            }
            if (/\s/.test(arg)) {
              return `"${props.long ? arg : truncate(arg, 30)}"`;
            }
            return arg;
          })
          .filter(notEmpty)
          .join(' ')}
    </code>
  );
}

export function CommandHistoryAndProgress() {
  const list = useRecoilValue(operationList);
  const queued = useRecoilValue(queuedOperations);
  const abortRunningOperation = useAbortRunningOperation();

  const info = useRecoilValue(repositoryInfo);
  if (info?.type !== 'success') {
    return null;
  }

  const progress = list.currentOperation;
  if (progress == null) {
    return null;
  }

  const desc = progress.operation.getDescriptionForDisplay();
  const command = (
    <OperationDescription
      info={info}
      operation={progress.operation}
      className="progress-container-command"
    />
  );

  let label;
  let icon;
  let abort = null;
  let showLastLineOfOutput = false;
  if (progress.exitCode == null) {
    label = desc?.description ? command : <T replace={{$command: command}}>Running $command</T>;
    icon = <Icon icon="loading" />;
    showLastLineOfOutput = desc?.tooltip == null;
    // Only show "Abort" for slow commands, since "Abort" might leave modified
    // files or pending commits around.
    const slowThreshold = 10000;
    const hideUntil = new Date((progress.startTime?.getTime() || 0) + slowThreshold);
    abort = (
      <Delayed hideUntil={hideUntil}>
        <VSCodeButton
          appearance="secondary"
          data-testid="abort-button"
          disabled={progress.aborting}
          onClick={() => {
            abortRunningOperation(progress.operation.id);
          }}>
          <Icon slot="start" icon={progress.aborting ? 'loading' : 'stop-circle'} />
          <T>Abort</T>
        </VSCodeButton>
      </Delayed>
    );
  } else if (progress.exitCode === 0) {
    label = <span>{command}</span>;
    icon = <Icon icon="pass" aria-label={t('Command exited successfully')} />;
  } else if (progress.aborting) {
    // Exited (tested above) by abort.
    label = <T replace={{$command: command}}>Aborted $command</T>;
    icon = <Icon icon="stop-circle" aria-label={t('Command aborted')} />;
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
            {desc?.tooltip || (
              <>
                <div className="progress-command-tooltip-command">
                  <strong>Command: </strong>
                  <OperationDescription info={info} operation={progress.operation} long />
                </div>
                <br />
                <b>Command output:</b>
                <br />
                <pre>{progress.commandOutput?.join('') || 'No output'}</pre>
              </>
            )}
          </div>
        )}>
        {queued.length > 0 ? (
          <div className="queued-operations-container" data-testid="queued-commands">
            <strong>Next to run</strong>
            {queued.map(op => (
              <div key={op.id} id={op.id} className="queued-operation">
                <OperationDescription info={info} operation={op} />
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
            <div className="progress-container-last-output">
              {progress.currentProgress != null ? (
                <ProgressLine
                  progress={progress.currentProgress.progress}
                  progressTotal={progress.currentProgress.progressTotal}>
                  {progress.currentProgress.message}
                </ProgressLine>
              ) : (
                progress.commandOutput
                  ?.slice(-1)
                  .map((line, i) => <ProgressLine key={i}>{line}</ProgressLine>)
              )}
            </div>
          </div>
        ) : null}
        {abort}
      </Tooltip>
    </div>
  );
}

function ProgressLine({
  children,
  progress,
  progressTotal,
}: {
  children: ReactNode;
  progress?: number;
  progressTotal?: number;
}) {
  return (
    <span className="progress-line">
      {progress != null && progressTotal != null ? (
        <ProgressBar progress={progress} progressTotal={progressTotal} />
      ) : null}
      <code>{children}</code>
    </span>
  );
}

function ProgressBar({progress, progressTotal}: {progress: number; progressTotal: number}) {
  const pct = progress / progressTotal;
  return (
    <span className="progress-bar">
      <span className="progress-bar-filled" style={{width: `${Math.round(100 * pct)}%`}} />
    </span>
  );
}
