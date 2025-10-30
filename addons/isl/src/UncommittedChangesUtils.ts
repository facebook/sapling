/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Set as ImSet} from 'immutable';
import type {ChangedFile, ChangedFileStatus, RepoRelativePath} from './types';
import type {UIChangedFile, VisualChangedFileStatus} from './UncommittedChanges';

import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {notEmpty} from 'shared/utils';
import {t} from './i18n';
import {ChangedFileMode} from './types';

export function processChangedFiles(
  files: Array<ChangedFile>,
  submodulePaths: ImSet<RepoRelativePath> | undefined,
): Array<UIChangedFile> {
  const disambiguousPaths = minimalDisambiguousPaths(files.map(file => file.path));
  const copySources = new Set(files.map(file => file.copy).filter(notEmpty));
  const removedFiles = new Set(files.filter(file => file.status === 'R').map(file => file.path));

  return (
    files
      .map((file, i) => {
        const minimalName = disambiguousPaths[i];
        const mode =
          submodulePaths && submodulePaths.has(file.path)
            ? ChangedFileMode.Submodule
            : ChangedFileMode.Regular;
        let fileLabel = minimalName;
        let tooltip = `${nameForStatus(file.status)}: ${file.path}`;
        let copiedFrom;
        let renamedFrom;
        let visualStatus: VisualChangedFileStatus = file.status;
        if (file.copy != null) {
          // Disambiguate between original file and the newly copy's name,
          // instead of disambiguating among all file names.
          const [originalName, copiedName] = minimalDisambiguousPaths([file.copy, file.path]);
          fileLabel = `${originalName} → ${copiedName}`;
          if (removedFiles.has(file.copy)) {
            renamedFrom = file.copy;
            tooltip = t('$newPath\n\nThis file was renamed from $originalPath', {
              replace: {$newPath: file.path, $originalPath: file.copy},
            });
            visualStatus = 'Renamed';
          } else {
            copiedFrom = file.copy;
            tooltip = t('$newPath\n\nThis file was copied from $originalPath', {
              replace: {$newPath: file.path, $originalPath: file.copy},
            });
            visualStatus = 'Copied';
          }
        }

        return {
          path: file.path,
          label: fileLabel,
          status: file.status,
          mode,
          visualStatus,
          copiedFrom,
          renamedFrom,
          tooltip,
        };
      })
      // Hide files that were renamed. This comes after the map since we need to use the index to refer to minimalDisambiguousPaths
      .filter(file => !(file.status === 'R' && copySources.has(file.path)))
      .sort((a, b) =>
        a.visualStatus === b.visualStatus
          ? a.path.localeCompare(b.path)
          : sortKeyForStatus[a.visualStatus] - sortKeyForStatus[b.visualStatus],
      )
  );
}

const sortKeyForStatus: Record<VisualChangedFileStatus, number> = {
  M: 0,
  Renamed: 1,
  A: 2,
  Copied: 3,
  R: 4,
  '!': 5,
  '?': 6,
  U: 7,
  Resolved: 8,
};

function nameForStatus(status: ChangedFileStatus): string {
  switch (status) {
    case '!':
      return t('Missing');
    case '?':
      return t('Untracked');
    case 'A':
      return t('Added');
    case 'M':
      return t('Modified');
    case 'R':
      return t('Removed');
    case 'U':
      return t('Unresolved');
    case 'Resolved':
      return t('Resolved');
  }
}
