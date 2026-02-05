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
import {useState, useEffect, useRef} from 'react';
import {notEmpty, truncate} from 'shared/utils';
import {WorktreeAddOperation} from './operations/WorktreeAddOperation';
import {Delayed} from './Delayed';
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

/** Minimum time to show the centered overlay to avoid jarring flashes */
const MIN_OVERLAY_DISPLAY_MS = 400;

/**
 * Hook to manage centered overlay visibility with minimum display time.
 * Prevents jarring flash for quick operations.
 */
function useCenteredOverlay(isRunning: boolean): boolean {
  const [showCentered, setShowCentered] = useState(false);
  const showStartTime = useRef<number | null>(null);

  useEffect(() => {
    if (isRunning) {
      // Operation started - show overlay immediately
      setShowCentered(true);
      showStartTime.current = Date.now();
    } else if (showCentered) {
      // Operation finished - wait for minimum time before hiding
      const elapsed = Date.now() - (showStartTime.current ?? 0);
      const remaining = Math.max(0, MIN_OVERLAY_DISPLAY_MS - elapsed);

      const timer = setTimeout(() => {
        setShowCentered(false);
        showStartTime.current = null;
      }, remaining);

      return () => clearTimeout(timer);
    }
  }, [isRunning, showCentered]);

  return showCentered;
}

export function CommandHistoryAndProgress() {
  const list = useAtomValue(operationList);
  const queued = useAtomValue(queuedOperations);
  const [queuedError, setQueuedError] = useAtom(queuedOperationsErrorAtom);
  const abortRunningOperation = useAbortRunningOperation();

  const [collapsed, setCollapsed] = useAtom(nextToRunCollapsedAtom);
  const [errorCollapsed, setErrorCollapsed] = useAtom(queueErrorCollapsedAtom);

  const info = useAtomValue(repositoryInfo);

  // Compute isRunning before early returns to satisfy React hooks rules
  const progress = list.currentOperation;
  const isRunning = progress != null && progress.exitCode == null;
  const showCentered = useCenteredOverlay(isRunning);

  if (!info) {
    return null;
  }

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
  const isWorktreeOperation = progress.operation instanceof WorktreeAddOperation;
  if (progress.exitCode == null) {
    label = desc?.description ? command : <T replace={{$command: command}}>Running $command</T>;
    icon = isWorktreeOperation ? <WorktreeBranchAnimation /> : <Icon icon="loading" />;
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

  let processedLines = processTerminalLines(progress.commandOutput ?? []);
  if (desc?.tooltip != null) {
    // Output might contain a JSON string not suitable for human reading.
    // Filter the line out.
    processedLines = processedLines.filter(line => !line.startsWith('{'));
  }

  return (
    <div className={`progress-container${showCentered ? ' progress-container-centered' : ''}`} data-testid="progress-container">
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

      <Tooltip
        component={() => (
          <div className="progress-command-tooltip">
            {desc?.tooltip || (
              <>
                <div className="progress-command-tooltip-command">
                  <strong>Command: </strong>
                  <OperationDescription info={info} operation={progress.operation} long />
                </div>
              </>
            )}
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
          </div>
        )}>
        <div className="progress-container-row">
          {icon}
          {label}
          {progress.warnings?.map(warning => (
            <Banner
              icon={<Icon icon="warning" color="yellow" />}
              alwaysShowButtons
              kind={BannerKind.warning}>
              <T replace={{$provider: warning}}>$provider</T>
            </Banner>
          ))}
        </div>
        {showLastLineOfOutput ? (
          <div className="progress-container-row">
            <div className="progress-container-last-output">
              {progress.currentProgress != null && progress.currentProgress.unit != null ? (
                <ProgressLine
                  progress={progress.currentProgress.progress}
                  progressTotal={progress.currentProgress.progressTotal}>
                  {progress.currentProgress.message +
                    ` - ${progress.currentProgress.progress}/${progress.currentProgress.progressTotal} ${progress.currentProgress.unit}`}
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

/**
 * Animated SVG showing branches growing - represents worktree creation.
 */
function WorktreeBranchAnimation() {
  return (
    <svg
      className="worktree-branch-animation"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      style={{width: 20, height: 20}}>
      {/* Main trunk */}
      <path
        className="branch-trunk"
        d="M12 20 L12 8"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {/* Left branch */}
      <path
        className="branch-left"
        d="M12 12 L6 6"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {/* Right branch - the new worktree */}
      <path
        className="branch-right"
        d="M12 12 L18 6"
        stroke="var(--signal-medium-fg, #58a6ff)"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {/* Node circles */}
      <circle cx="12" cy="20" r="2" fill="currentColor" />
      <circle cx="12" cy="12" r="2" fill="currentColor" />
      <circle cx="6" cy="6" r="1.5" fill="currentColor" />
      <circle className="node-right-pulse" cx="18" cy="6" r="2" fill="var(--signal-medium-fg, #58a6ff)" />
    </svg>
  );
}
