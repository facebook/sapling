/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {tracker} from './analytics';
import {File} from './ChangedFile';
import {Column, Row} from './ComponentUtils';
import {T, t} from './i18n';
import {Internal} from './Internal';
import {readAtom, writeAtom} from './jotaiUtils';
import type {PartialSelection} from './partialSelection';
import platform from './platform';
import {repoRootAtom} from './repositoryData';
import type {AbsolutePath, RepoRelativePath} from './types';
import {ChangedFileMode} from './types';
import {showModal} from './useModal';
import {registerDisposable} from './utils';

import './UncommittedChanges.css';

/** All known suggested edits, if applicable.
 * n.b. we get absolute paths from the suggested edits API */
const allSuggestedEdits = atom<Array<AbsolutePath>>([]);
registerDisposable(
  allSuggestedEdits,
  platform.suggestedEdits?.onDidChangeSuggestedEdits(edits => {
    writeAtom(allSuggestedEdits, edits);
  }) ?? {dispose: () => {}},
  import.meta.hot,
);

/** Filter all known suggested edits to relevant repo-relative paths */
const currentSuggestedEdits = atom<Array<RepoRelativePath>>(get => {
  const allEdits = get(allSuggestedEdits);
  const repoRoot = get(repoRootAtom);
  return allEdits
    .filter(path => path.startsWith(repoRoot))
    .map(path => path.slice(repoRoot.length + 1));
});

/**
 * If there are pending suggested edits as determined by the platform (typically suggestions from an AI),
 * and they intersect with the given files,
 * show a modal to confirm how to resolve those edits before proceeding.
 * Different operations may have a different behavior for resolving.
 * For example, `commit` should accept the edits before continuing,
 * but `revert` should reject the edits before continuing.
 *
 * We intentionally don't expose all possible ways of resolving edits for simplicity as a user.
 * We don't give any option to leave edits pending, because that should almost never be what you want.
 *
 * `source` is used for analytics purposes.
 */
export async function confirmSuggestedEditsForFiles(
  source: string,
  action: 'accept' | 'reject',
  files: PartialSelection | Array<RepoRelativePath>,
): Promise<boolean> {
  const suggestedEdits = readAtom(currentSuggestedEdits);
  if (suggestedEdits == null || suggestedEdits.length === 0) {
    return true; // nothing to warn about
  }

  const toWarnAbout =
    files == null
      ? suggestedEdits
      : Array.isArray(files)
        ? suggestedEdits.filter(filepath => files.includes(filepath))
        : suggestedEdits.filter(filepath => files.isFullyOrPartiallySelected(filepath));
  if (toWarnAbout.length === 0) {
    return true; // nothing to warn about
  }

  const buttons = [
    t('Cancel'),
    action === 'accept'
      ? {label: t('Accept Edits and Continue'), primary: true}
      : {label: t('Discard Edits and Continue'), primary: true},
  ];
  const answer = await showModal({
    type: 'confirm',
    buttons,
    title: Internal.PendingSuggestedEditsMessage ?? <T>You have pending suggested edits</T>,
    message: (
      <Column alignStart>
        <Column alignStart>
          <SimpleChangedFilesList files={toWarnAbout} />
        </Column>
        <Row>
          {action === 'accept' ? (
            <T>Do you want to accept these suggested edits and continue?</T>
          ) : (
            <T>Do you want to discard these suggested edits and continue?</T>
          )}
        </Row>
      </Column>
    ),
  });
  tracker.track('WarnAboutSuggestedEdits', {
    extras: {
      source,
      answer: typeof answer === 'string' ? answer : answer?.label,
    },
  });

  switch (answer) {
    default:
    case buttons[0]:
      return false;
    case buttons[1]: {
      const fullEdits = readAtom(allSuggestedEdits);
      const absolutePaths = fullEdits.filter(path =>
        toWarnAbout.find(filepath => path.endsWith(filepath)),
      );
      platform.suggestedEdits?.resolveSuggestedEdits(action, absolutePaths);
      return true;
    }
  }
}

/** Simplified list of changed files, for rendering a list of files when we don't have the full context of the file.
 * Just pretend everything is modified and hide extra actions like opening diff views.
 */
function SimpleChangedFilesList({files}: {files: Array<string>}) {
  const disambiguated = minimalDisambiguousPaths(files);
  return (
    <div className="changed-files-list-container">
      <div className="changed-files-list">
        {files.map((path, i) => (
          <File
            file={{
              label: disambiguated[i],
              path,
              tooltip: path,
              // These are wrong, but we don't have the full context of the file to know if it's added, removed, etc
              visualStatus: 'M',
              status: 'M',
              // Similar to the above, we assume it's a regular change
              // rather than a submodule update, which is unlikely to be suggested
              mode: ChangedFileMode.Regular,
            }}
            key={path}
            displayType="short"
          />
        ))}
      </div>
    </div>
  );
}
