/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFilesDisplayType} from './ChangedFileDisplayTypePicker';
import type {Place, UIChangedFile} from './UncommittedChanges';
import type {UseUncommittedSelection} from './partialSelection';
import type {PathTree} from './pathTree';
import type {Comparison} from 'shared/Comparison';

import {File} from './UncommittedChanges';
import {buildPathTree} from './pathTree';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useMemo, useState} from 'react';
import {Icon} from 'shared/Icon';

type HeaderProps = {
  isCollapsed: boolean;
  toggleCollapsed: () => void;
  folder: string;
};

export function FileTreeFolderHeader({isCollapsed, toggleCollapsed, folder}: HeaderProps) {
  return (
    <span className="file-tree-folder-path">
      <VSCodeButton appearance="icon" onClick={toggleCollapsed}>
        <Icon icon={isCollapsed ? 'chevron-right' : 'chevron-down'} slot="start" />
        {folder}
      </VSCodeButton>
    </span>
  );
}
export function FileTree(props: {
  files: Array<UIChangedFile>;
  displayType: ChangedFilesDisplayType;
  comparison: Comparison;
  selection?: UseUncommittedSelection;
  place?: Place;
}) {
  const {files, ...rest} = props;

  const tree = useMemo(
    () => buildPathTree(Object.fromEntries(files.map(file => [file.path, file]))),
    [files],
  );

  const [collapsed, setCollapsed] = useState(new Set());

  function renderTree(tree: PathTree<UIChangedFile>, accumulatedPath = '') {
    return (
      <div className="file-tree">
        {[...tree.entries()].map(([folder, inner]) => {
          const folderKey = `${accumulatedPath}/${folder}`;
          const isCollapsed = collapsed.has(folderKey);
          return (
            <div className="file-tree-level" key={folderKey}>
              {inner instanceof Map ? (
                <>
                  <FileTreeFolderHeader
                    isCollapsed={isCollapsed}
                    toggleCollapsed={() => {
                      setCollapsed(last =>
                        isCollapsed
                          ? new Set([...last].filter(v => v !== folderKey))
                          : new Set([...last, folderKey]),
                      );
                    }}
                    folder={folder}
                  />
                  {isCollapsed ? null : renderTree(inner, folderKey)}
                </>
              ) : (
                <File key={inner.path} {...rest} file={inner} />
              )}
            </div>
          );
        })}
      </div>
    );
  }

  return renderTree(tree);
}
