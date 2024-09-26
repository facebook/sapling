/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UseUncommittedSelection} from './partialSelection';
import type {CommitInfo, Diagnostic} from './types';

import {spacing} from '../../components/theme/tokens.stylex';
import serverAPI from './ClientToServerAPI';
import {Collapsable} from './Collapsable';
import {Internal} from './Internal';
import {tracker} from './analytics';
import {getFeatureFlag} from './featureFlags';
import {T, t} from './i18n';
import {localStorageBackedAtom, readAtom} from './jotaiUtils';
import foundPlatform from './platform';
import {uncommittedChangesWithPreviews} from './previews';
import {showModal} from './useModal';
import * as stylex from '@stylexjs/stylex';
import {Checkbox} from 'isl-components/Checkbox';
import {Column, Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';
import {basename} from 'shared/utils';

export const shouldWarnAboutDiagnosticsAtom = localStorageBackedAtom<boolean>(
  'isl.warn-about-diagnostics',
  true,
);

const hideNonBlockingDiagnosticsAtom = localStorageBackedAtom<boolean>(
  'isl.hide-non-blocking-diagnostics',
  true,
);

const styles = stylex.create({
  diagnosticList: {
    paddingInline: spacing.double,
    paddingBlock: spacing.half,
    gap: 0,
  },
  nowrap: {
    whiteSpace: 'nowrap',
  },
  diagnosticRow: {
    maxWidth: 'max(400px, 80vw)',
    padding: spacing.half,
    cursor: 'pointer',
    ':hover': {
      backgroundColor: 'var(--hover-darken)',
    },
  },
  allDiagnostics: {
    maxHeight: '80vh',
    overflowY: 'scroll',
  },
  confirmCheckbox: {
    paddingTop: spacing.double,
  },
});

/**
 * Check IDE diagnostics for files that will be commit/amended/submitted,
 * to confirm if they intended the errors.
 */
export async function confirmNoBlockingDiagnostics(
  /** Check diagnostics for these selected files. */
  selection: UseUncommittedSelection,
  /** If provided, warn for changes to files in this commit. Used when checking diagnostics when amending a commit. */
  commit?: CommitInfo,
): Promise<boolean> {
  if (!readAtom(shouldWarnAboutDiagnosticsAtom)) {
    return true;
  }
  if (foundPlatform.platformName === 'vscode') {
    const allFiles = new Set<string>();
    for (const file of readAtom(uncommittedChangesWithPreviews)) {
      if (selection.isFullyOrPartiallySelected(file.path)) {
        allFiles.add(file.path);
      }
    }
    for (const file of commit?.filesSample ?? []) {
      allFiles.add(file.path);
    }

    serverAPI.postMessage({
      type: 'platform/checkForDiagnostics',
      paths: [...allFiles],
    });
    const [result, enabled] = await Promise.all([
      serverAPI.nextMessageMatching('platform/gotDiagnostics', () => true),
      getFeatureFlag(
        Internal.featureFlags?.ShowPresubmitDiagnosticsWarning,
        /* enable this feature in OSS */ true,
      ),
    ]);
    if (result.diagnostics.size > 0) {
      const allDiagnostics = [...result.diagnostics.values()];
      const totalErrors = allDiagnostics
        .map(value => value.filter(d => d.severity === 'error').length)
        .reduce((a, b) => a + b, 0);

      const allSources = [...new Set(allDiagnostics.flat().map(d => d.source))];
      const totalDiagnostics = allDiagnostics.flat().length;

      const childTracker = tracker.trackAsParent('DiagnosticsConfirmationOpportunity', {
        extras: {
          shown: enabled,
          sources: allSources,
          totalErrors,
          totalDiagnostics,
        },
      });

      if (!enabled) {
        return true;
      }

      if (totalErrors > 0) {
        const buttons = [{label: 'Cancel'}, {label: 'Continue', primary: true}] as const;
        const shouldContinue =
          (await showModal({
            type: 'confirm',
            title: t('codeIssuesFound', {count: totalErrors}),
            message: <DiagnosticsList diagnostics={[...result.diagnostics.entries()]} />,
            buttons,
          })) === buttons[1];

        childTracker.track('DiagnosticsConfirmationAction', {
          extras: {
            action: shouldContinue ? 'continue' : 'cancel',
          },
        });

        return shouldContinue;
      }
    }
  }
  return true;
}

function DiagnosticsList({diagnostics}: {diagnostics: Array<[string, Array<Diagnostic>]>}) {
  const [hideNonBlocking, setHideNonBlocking] = useAtom(hideNonBlockingDiagnosticsAtom);
  const [shouldWarn, setShouldWarn] = useAtom(shouldWarnAboutDiagnosticsAtom);
  return (
    <>
      <Column alignStart xstyle={styles.allDiagnostics}>
        {diagnostics.map(([filepath, diagnostics]) => {
          const sortedDiagnostics = [...diagnostics]
            .filter(d => (hideNonBlocking ? d.severity === 'error' : true))
            .sort((a, b) => {
              return severityComparator(a) - severityComparator(b);
            });
          return (
            <Column key={filepath} alignStart>
              <Collapsable
                startExpanded
                title={
                  <Row>
                    <span>{basename(filepath)}</span>
                    <Subtle>{filepath}</Subtle>
                  </Row>
                }>
                <Column alignStart xstyle={styles.diagnosticList}>
                  {sortedDiagnostics.map(d => (
                    <Row
                      role="button"
                      tabIndex={0}
                      key={d.source}
                      xstyle={styles.diagnosticRow}
                      onClick={() => {
                        foundPlatform.openFile(filepath, {line: d.range.startLine + 1});
                      }}>
                      {iconForDiagnostic(d)}
                      <span>{d.message}</span>
                      {d.source && (
                        <Subtle {...stylex.props(styles.nowrap)}>
                          {d.source}
                          {d.code ? `(${d.code})` : null}
                        </Subtle>
                      )}{' '}
                      <Subtle {...stylex.props(styles.nowrap)}>
                        [Ln {d.range.startLine}, Col {d.range.startCol}]
                      </Subtle>
                    </Row>
                  ))}
                </Column>
              </Collapsable>
            </Column>
          );
        })}
      </Column>
      <Row xstyle={styles.confirmCheckbox}>
        <Checkbox checked={!shouldWarn} onChange={checked => setShouldWarn(!checked)}>
          <T>Don't ask again</T>
        </Checkbox>
        <Tooltip
          title={t(
            "Only 'error' severity issues will cause this dialog to appear, but less severe issues can still be shown here. This option hides these non-blocking issues.",
          )}>
          <Checkbox checked={hideNonBlocking} onChange={setHideNonBlocking}>
            <T>Hide non-blocking issues</T>
          </Checkbox>
        </Tooltip>
      </Row>
    </>
  );
}

function severityComparator(a: Diagnostic) {
  switch (a.severity) {
    case 'error':
      return 0;
    case 'warning':
      return 1;
    case 'info':
      return 2;
    case 'hint':
      return 3;
  }
}

function iconForDiagnostic(d: Diagnostic): React.ReactNode {
  switch (d.severity) {
    case 'error':
      return <Icon icon="error" color="red" />;
    case 'warning':
      return <Icon icon="warning" color="yellow" />;
    case 'info':
      return <Icon icon="info" />;
    case 'hint':
      return <Icon icon="info" />;
  }
}
