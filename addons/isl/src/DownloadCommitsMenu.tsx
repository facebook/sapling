/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Divider} from 'isl-components/Divider';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';
import {useEffect, useRef, useState} from 'react';
import {nullthrows} from 'shared/utils';
import {Collapsable} from './Collapsable';
import {CommitCloudInfo} from './CommitCloud';
import {DropdownFields} from './DropdownFields';
import {GotoTimeContent} from './GotoTimeMenu';
import {useCommandEvent} from './ISLShortcuts';
import {Internal} from './Internal';
import {tracker} from './analytics';
import {findPublicBaseAncestor} from './getCommitTree';
import {t, T} from './i18n';
import {configBackedAtom, readAtom} from './jotaiUtils';
import {GotoOperation} from './operations/GotoOperation';
import {GraftOperation} from './operations/GraftOperation';
import {PullRevOperation} from './operations/PullRevOperation';
import {RebaseKeepOperation} from './operations/RebaseKeepOperation';
import {RebaseOperation} from './operations/RebaseOperation';
import {useRunOperation} from './operationsState';
import {dagWithPreviews} from './previews';
import {forceFetchCommit} from './serverAPIState';
import {exactRevset, succeedableRevset} from './types';

import './DownloadCommitsMenu.css';

export function DownloadCommitsTooltipButton() {
  const additionalToggles = useCommandEvent('ToggleDownloadCommitsDropdown');
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <DownloadCommitsTooltip dismiss={dismiss} />}
      placement="bottom"
      additionalToggles={additionalToggles.asEventTarget()}
      group="topbar"
      title={
        <div>
          <T replace={{$shortcut: <Kbd modifiers={[Modifier.ALT]} keycode={KeyCode.D} />}}>
            Download commits and diffs ($shortcut)
          </T>
        </div>
      }>
      <Button icon data-testid="download-commits-tooltip-button">
        <Icon icon="cloud-download" />
      </Button>
    </Tooltip>
  );
}

const downloadCommitRebaseType = configBackedAtom<'rebase_base' | 'rebase_ontop' | null>(
  'isl.download-commit-rebase-type',
  null,
);

const downloadCommitShouldGoto = configBackedAtom<boolean>(
  'isl.download-commit-should-goto',
  false,
);

function maybeSupportedByPull(name: string): boolean {
  return (
    !name.includes('(') /* likely a revset */ &&
    name.length < 42 /* likely an expression or rREPOhash Phabricator callsign */
  );
}

function DownloadCommitsTooltip({dismiss}: {dismiss: () => unknown}) {
  const [enteredRevset, setEnteredRevset] = useState('');
  const runOperation = useRunOperation();
  const supportsDiffDownload = Internal.diffDownloadOperation != null;
  const downloadDiffTextArea = useRef(null);
  useEffect(() => {
    if (downloadDiffTextArea.current) {
      (downloadDiffTextArea.current as HTMLTextAreaElement).focus();
    }
  }, [downloadDiffTextArea]);

  const [rebaseType, setRebaseType] = useAtom(downloadCommitRebaseType);
  const [shouldGoto, setShouldGoto] = useAtom(downloadCommitShouldGoto);

  const doCommitDownload = async () => {
    tracker.track('ClickPullButton', {
      extras: {
        rebaseType,
        shouldGoto,
      },
    });

    // We need to dismiss the tooltip now, since we don't want to leave it up until after the operations are run.
    dismiss();

    // Typically, we'd just immediately use runOperation to queue up additional operations.
    // Unfortunately, we don't know if the result will be public or not,
    // and that changes how we'll rebase/graft the result.  This means we can't use the queueing system.
    // This is not a correctness issue because we show no optimistically downloaded result to act on.
    // Worst case, the rebase/goto will be queued after some other unrelated actions which should be fine.

    if (maybeSupportedByPull(enteredRevset)) {
      try {
        await runOperation(
          new PullRevOperation(exactRevset(enteredRevset)),
          /* throwOnError */ true,
        );
      } catch (err) {
        if (Internal.diffDownloadOperation != null) {
          // Note: try backup diff download system internally
          await runOperation(
            Internal.diffDownloadOperation(exactRevset(enteredRevset)),
            /* throwOnError */ true,
          );
        } else {
          // If there's no backup operation, respect the error and don't try further actions
          throw err;
        }
      }
    }

    // Lookup the result of the pull
    const latest = await forceFetchCommit(enteredRevset).catch(() => null);
    if (!latest) {
      // We can't continue with the rebase/goto if the lookup failed.
      return;
    }

    // Now we CAN queue up additional actions

    const isPublic = latest?.phase === 'public';
    if (rebaseType != null) {
      const Op = isPublic
        ? // "graft" implicitly does "goto", "rebase --keep" does not
          shouldGoto
          ? GraftOperation
          : RebaseKeepOperation
        : RebaseOperation;
      const dest =
        rebaseType === 'rebase_ontop'
          ? '.'
          : nullthrows(findPublicBaseAncestor(readAtom(dagWithPreviews))?.hash);
      // Use exact revsets for sources, so that you can type a specific hash to download and not be surprised by succession.
      // Only use succession for destination, which may be in flux at the moment you start the download.
      runOperation(new Op(exactRevset(enteredRevset), succeedableRevset(dest)));
    }

    if (
      shouldGoto &&
      // Goto for public commits will be handled by Graft, if a rebase/graft was performed.
      // Goto on max(latest_successors(revset)) would just yield the existing public commit,
      // but for non-landed commits, using succeedableRevset allows goto the newly rebased commit.
      // If no rebase was performed, we will use goto even for public commits.
      (!isPublic || rebaseType === null)
    ) {
      runOperation(
        new GotoOperation(
          // if not rebasing, just use the exact revset.
          rebaseType == null ? exactRevset(enteredRevset) : succeedableRevset(enteredRevset),
        ),
      );
    }
  };

  return (
    <DropdownFields
      title={<T>Download Commits</T>}
      icon="cloud-download"
      data-testid="download-commits-dropdown">
      <div className="download-commits-content">
        <div className="download-commits-input-row">
          <TextField
            width="100%"
            placeholder={
              supportsDiffDownload ? t('Hash, Diff Number, ...') : t('Hash, revset, pr123, ...')
            }
            value={enteredRevset}
            data-testid="download-commits-input"
            onInput={e => setEnteredRevset((e.target as unknown as {value: string})?.value ?? '')}
            onKeyDown={e => {
              if (e.key === 'Enter') {
                if (enteredRevset.trim().length > 0) {
                  doCommitDownload();
                }
              }
            }}
            ref={downloadDiffTextArea}
          />
          <Button
            data-testid="download-commit-button"
            disabled={enteredRevset.trim().length === 0}
            onClick={doCommitDownload}>
            <T>Pull</T>
          </Button>
        </div>
        <div className="download-commits-input-row">
          <Tooltip title={t('After downloading this commit, also go there')}>
            <Checkbox checked={shouldGoto} onChange={setShouldGoto}>
              <T>Go to</T>
            </Checkbox>
          </Tooltip>
          <Tooltip
            title={t(
              'After downloading this commit, rebase it onto the public base of the current stack. Public commits will be copied instead of moved.',
            )}>
            <Checkbox
              checked={rebaseType === 'rebase_base'}
              onChange={checked => {
                setRebaseType(checked ? 'rebase_base' : null);
              }}>
              <T>Rebase to Stack Base</T>
            </Checkbox>
          </Tooltip>
          <Tooltip
            title={t(
              'After downloading this commit, rebase it on top of the current commit. Public commits will be copied instead of moved.',
            )}>
            <Checkbox
              checked={rebaseType === 'rebase_ontop'}
              onChange={checked => {
                setRebaseType(checked ? 'rebase_ontop' : null);
              }}>
              <T>Rebase onto Stack</T>
            </Checkbox>
          </Tooltip>
        </div>
      </div>

      <Collapsable
        title={
          <>
            <Icon icon="clock" />
            <T>Go to time</T>
          </>
        }
        className="download-commits-expander">
        <GotoTimeContent dismiss={dismiss} />
      </Collapsable>

      {Internal.supportsCommitCloud && (
        <>
          <Divider />
          <CommitCloudInfo />
        </>
      )}
    </DropdownFields>
  );
}
