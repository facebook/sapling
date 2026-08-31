/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Operation} from './operations/Operation';
import type {ValidatedRepoInfo} from './types';

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Column, Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useCallback, useEffect, useRef, useState} from 'react';
import {notEmpty, truncate} from 'shared/utils';
import {LogRenderExposures} from './analytics/LogRenderExposures';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {
  EXIT_CODE_FORGET,
  operationList,
  queuedOperations,
  queuedOperationsErrorAtom,
  useAbortRunningOperation,
} from './operationsState';
import {repositoryInfo} from './serverAPIState';
import {processTerminalLines} from './terminalOutput';
import {CommandRunner} from './types';
import {short} from './utils';

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
      ? (/[^\\/]+$/.exec(info.command)?.[0] ?? 'sl')
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
const queueErrorCollapsedAtom = atom(true);

export function CommandHistoryAndProgress() {
  const list = useAtomValue(operationList);
  const queued = useAtomValue(queuedOperations);
  const [queuedError, setQueuedError] = useAtom(queuedOperationsErrorAtom);
  const abortRunningOperation = useAbortRunningOperation();

  const [collapsed, setCollapsed] = useAtom(nextToRunCollapsedAtom);
  const [errorCollapsed, setErrorCollapsed] = useAtom(queueErrorCollapsedAtom);

  const [outputExpanded, setOutputExpanded] = useState(false);

  const outputRef = useRef<HTMLDivElement>(null);
  const userScrolledUpRef = useRef(false);

  const info = useAtomValue(repositoryInfo);
  if (!info) {
    return null;
  }

  const progress = list.currentOperation;
  if (progress == null) {
    return null;
  }

  const isRunning = progress.exitCode == null;
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
  let showLastLineOfOutput = false;
  if (isRunning) {
    label = desc?.description ? command : <T replace={{$command: command}}>Running $command</T>;
    icon = <Icon icon="loading" />;
    showLastLineOfOutput = desc?.tooltip == null;
  } else if (progress.exitCode === 0) {
    label = <span>{command}</span>;
    icon = <Icon icon="pass" aria-label={t('Command exited successfully')} />;
  } else if (progress.aborting) {
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

  let processedLines = processTerminalLines(progress.commandOutput ?? []);
  if (desc?.tooltip != null) {
    processedLines = processedLines.filter(line => !line.startsWith('{'));
  }

  const showAbort = isRunning;

  const lastLine =
    showLastLineOfOutput && processedLines.length > 0 ? processedLines.at(-1) : undefined;

  const progressInfo =
    isRunning &&
    showLastLineOfOutput &&
    progress.currentProgress != null &&
    progress.currentProgress.unit != null
      ? progress.currentProgress
      : undefined;

  return (
    <div className="progress-container" data-testid="progress-container">
      {queuedError != null || queued.length > 0 ? (
        <div className="queued-operations-container" data-testid="queued-commands">
          {queuedError != null && (
            <LogRenderExposures eventName="QueueCancelledWarningShown">
              <Column alignStart data-testid="cancelled-queued-commands">
                <Tooltip
                  title={t(
                    'When an operation process fails or is aborted, any operations queued after that are cancelled, as they may depend on the previous operation succeeding.',
                  )}>
                  <Row
                    style={{cursor: 'pointer'}}
                    onClick={() => {
                      setErrorCollapsed(!errorCollapsed);
                    }}>
                    <Icon icon={errorCollapsed ? 'chevron-right' : 'chevron-down'} />
                    <Banner kind={BannerKind.warning}>
                      <Icon icon="warning" color="yellow" />
                      <T count={queuedError.operations.length}>queuedOperationsWereCancelled</T>
                    </Banner>
                    <Tooltip title={t('Dismiss')}>
                      <Button
                        icon
                        onClick={() => {
                          setQueuedError(undefined);
                        }}>
                        <Icon icon="x" />
                      </Button>
                    </Tooltip>
                  </Row>
                </Tooltip>
                {errorCollapsed ? null : (
                  <TruncatedOperationList operations={queuedError.operations} info={info} />
                )}
              </Column>
            </LogRenderExposures>
          )}
          {queued.length > 0 ? (
            <>
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
                <TruncatedOperationList operations={queued} info={info} />
              )}
            </>
          ) : null}
        </div>
      ) : null}

      <div className="progress-collapsible">
        <div
          className="progress-header-row"
          onClick={() => setOutputExpanded(prev => !prev)}
          data-testid="progress-header-row">
          <Icon icon={outputExpanded ? 'chevron-down' : 'chevron-right'} />
          {icon}
          {label}
          {progress.warnings?.map(warning => (
            <Banner
              key={warning}
              icon={<Icon icon="warning" color="yellow" />}
              alwaysShowButtons
              kind={BannerKind.warning}>
              <T replace={{$provider: warning}}>$provider</T>
            </Banner>
          ))}
          {progressInfo != null ? (
            <span className="progress-header-last-line">
              <ProgressLine
                progress={progressInfo.progress}
                progressTotal={progressInfo.progressTotal}>
                {progressInfo.message +
                  ` - ${progressInfo.progress}/${progressInfo.progressTotal} ${progressInfo.unit}`}
              </ProgressLine>
            </span>
          ) : !outputExpanded && lastLine != null ? (
            <span className="progress-header-last-line">
              <code>{lastLine}</code>
            </span>
          ) : null}
          {showAbort && (
            <span className="progress-abort-wrapper">
              <Tooltip title={progress.aborting ? t('Aborting…') : t('Abort')}>
                <Button
                  icon
                  data-testid="abort-button"
                  className="progress-abort-button"
                  disabled={progress.aborting}
                  onClick={e => {
                    e.stopPropagation();
                    abortRunningOperation(progress.operation.id);
                  }}>
                  <Icon icon={progress.aborting ? 'loading' : 'debug-stop'} />
                </Button>
              </Tooltip>
            </span>
          )}
        </div>

        {outputExpanded && (
          <CommandOutputArea
            processedLines={processedLines}
            isRunning={isRunning}
            outputRef={outputRef}
            userScrolledUpRef={userScrolledUpRef}
          />
        )}
      </div>
    </div>
  );
}

function CommandOutputArea({
  processedLines,
  isRunning,
  outputRef,
  userScrolledUpRef,
}: {
  processedLines: string[];
  isRunning: boolean;
  outputRef: React.RefObject<HTMLDivElement | null>;
  userScrolledUpRef: React.MutableRefObject<boolean>;
}) {
  const scrollToBottom = useCallback(() => {
    const el = outputRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [outputRef]);

  useEffect(() => {
    if (!userScrolledUpRef.current) {
      scrollToBottom();
    }
  }, [processedLines.length, scrollToBottom, userScrolledUpRef]);

  const handleScroll = useCallback(() => {
    const el = outputRef.current;
    if (!el) {
      return;
    }
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 10;
    userScrolledUpRef.current = !atBottom;
  }, [outputRef, userScrolledUpRef]);

  return (
    <div
      className="progress-output-area"
      ref={outputRef}
      onScroll={handleScroll}
      data-testid="progress-output">
      {processedLines.length === 0 ? (
        <Subtle>{isRunning ? <T>Waiting for output…</T> : <T>No output</T>}</Subtle>
      ) : (
        <pre className="progress-output-pre">
          {processedLines.map((line, i) => (
            <div key={i}>{line}</div>
          ))}
        </pre>
      )}
    </div>
  );
}

const MAX_VISIBLE_NEXT_TO_RUN = 10;
function TruncatedOperationList({
  info,
  operations,
}: {
  info: ValidatedRepoInfo;
  operations: Array<Operation>;
}) {
  return (
    <>
      {(operations.length > MAX_VISIBLE_NEXT_TO_RUN
        ? operations.slice(0, MAX_VISIBLE_NEXT_TO_RUN)
        : operations
      ).map(op => (
        <div key={op.id} id={op.id} className="queued-operation">
          <OperationDescription info={info} operation={op} />
        </div>
      ))}
      {operations.length > MAX_VISIBLE_NEXT_TO_RUN && (
        <div>
          <T replace={{$count: operations.length - MAX_VISIBLE_NEXT_TO_RUN}}>+$count more</T>
        </div>
      )}
    </>
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
