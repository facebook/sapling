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

import {File} from './ChangedFile';
import {Button} from './components/Button';
import {Checkbox} from './components/Checkbox';
import {buildPathTree} from './pathTree';
import {useMemo, useState} from 'react';
import {Icon} from 'shared/Icon';

export function FileTreeFolderHeader({
  isCollapsed,
  toggleCollapsed,
  checkedState,
  toggleChecked,
  folder,
}: {
  isCollapsed: boolean;
  toggleCollapsed: () => void;
  checkedState?: true | false | 'indeterminate';
  toggleChecked?: (checked: boolean) => void;
  folder: string;
}) {
  return (
    <span className="file-tree-folder-path">
      {checkedState != null && toggleChecked != null && (
        <Checkbox
          checked={checkedState === true}
          indeterminate={checkedState === 'indeterminate'}
          onChange={() => toggleChecked(checkedState !== true)}
        />
      )}
      <Button icon onClick={toggleCollapsed}>
        <Icon icon={isCollapsed ? 'chevron-right' : 'chevron-down'} slot="start" />
        {folder}
      </Button>
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
