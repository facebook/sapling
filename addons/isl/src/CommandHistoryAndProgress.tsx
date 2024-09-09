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
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {
  EXIT_CODE_FORGET,
  operationList,
  queuedOperations,
  useAbortRunningOperation,
} from './operationsState';
import {repositoryInfo} from './serverAPIState';
import {processTerminalLines} from './terminalOutput';
import {CommandRunner} from './types';
import {short} from './utils';
import {Button} from 'isl-components/Button';
import {Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import {notEmpty, truncate} from 'shared/utils';

import './CommandHistoryAndProgress.css';

function OperationDescription(props: {
  info: ValidatedRepoInfo;
  operation: Operation;
  className?: string;
  long?: boolean;
}): React.ReactElement {
  const {info, operation, className} = props;
  const desc = operation.getDescriptionForDisplay();

  const reviewProvider = useAtomValue(codeReviewProvider);

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
                case 'repo-relative-file-list':
                  return truncate(arg.paths.join(' '), 200);
                case 'exact-revset':
                case 'succeedable-revset':
                case 'optimistic-revset':
                  return props.long
                    ? arg.revset
                    : // truncate full commit hashes to short representation visually
                    // revset could also be a remote bookmark, so only do this if it looks like a hash
                    /^[a-z0-9]{40}$/.test(arg.revset)
                    ? short(arg.revset)
                    : truncate(arg.revset, 80);
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

const nextToRunCollapsedAtom = atom(false);

export function CommandHistoryAndProgress() {
  const list = useAtomValue(operationList);
  const queued = useAtomValue(queuedOperations);
  const abortRunningOperation = useAbortRunningOperation();

  const [collapsed, setCollapsed] = useAtom(nextToRunCollapsedAtom);

  const info = useAtomValue(repositoryInfo);
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
        <Button
          data-testid="abort-button"
          disabled={progress.aborting}
          onClick={() => {
            abortRunningOperation(progress.operation.id);
          }}>
          <Icon slot="start" icon={progress.aborting ? 'loading' : 'stop-circle'} />
          <T>Abort</T>
        </Button>
      </Delayed>
    );
  } else if (progress.exitCode === 0) {
    label = <span>{command}</span>;
    icon = <Icon icon="pass" aria-label={t('Command exited successfully')} />;
  } else if (progress.aborting) {
    // Exited (tested above) by abort.
    label = <T replace={{$command: command}}>Aborted $command</T>;
    icon = <Icon icon="stop-circle" aria-label={t('Command aborted')} />;
  } else if (progress.exitCode === EXIT_CODE_FORGET) {
    label = <span>{command}</span>;
    icon = (
      <Icon
        icon="question"
        aria-label={t('Command ran during disconnection. Exit status is lost.')}
      />
    );
  } else {
    label = <span>{command}</span>;
    icon = <Icon icon="error" aria-label={t('Command exited unsuccessfully')} />;
    showLastLineOfOutput = true;
  }

  const processedLines = processTerminalLines(progress.commandOutput ?? []);

  const MAX_VISIBLE_NEXT_TO_RUN = 10;

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
                {processedLines.length === 0 ? (
                  <Subtle>
                    <T>No output</T>
                  </Subtle>
                ) : (
                  <pre>
                    {processedLines.map((line, i) => (
                      <div key={i}>{line}</div>
                    ))}
                  </pre>
                )}
              </>
            )}
          </div>
        )}>
        {queued.length > 0 ? (
          <div className="queued-operations-container" data-testid="queued-commands">
            <Row
              style={{cursor: 'pointer'}}
              onClick={() => {
                setCollapsed(!collapsed);
              }}>
              <Icon icon={collapsed ? 'chevron-right' : 'chevron-down'} />
              <strong>
                <T>Next to run</T>
              </strong>
            </Row>
            {collapsed ? (
              <div>
                <T count={queued.length}>moreCommandsToRun</T>
              </div>
            ) : (
              <>
                {(queued.length > MAX_VISIBLE_NEXT_TO_RUN
                  ? queued.slice(0, MAX_VISIBLE_NEXT_TO_RUN)
                  : queued
                ).map(op => (
                  <div key={op.id} id={op.id} className="queued-operation">
                    <OperationDescription info={info} operation={op} />
                  </div>
                ))}
                {queued.length > MAX_VISIBLE_NEXT_TO_RUN && (
                  <div>
                    <T replace={{$count: queued.length - MAX_VISIBLE_NEXT_TO_RUN}}>+$count more</T>
                  </div>
                )}
              </>
            )}
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
                processedLines.length > 0 && <ProgressLine>{processedLines.at(-1)}</ProgressLine>
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
