/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';
import type {ChangedFilesDisplayType} from './ChangedFileDisplayTypePicker';
import type {Place, UIChangedFile} from './UncommittedChanges';
import type {UseUncommittedSelection} from './partialSelection';
import type {PathTree} from './pathTree';

import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Icon} from 'isl-components/Icon';
import {useMemo, useState} from 'react';
import {mapIterable} from 'shared/utils';
import {File} from './ChangedFile';
import {buildPathTree, calculateTreeSelectionStates} from './pathTree';

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

function* iteratePathTree(tree: PathTree<UIChangedFile>): Generator<UIChangedFile> {
  for (const node of tree.values()) {
    if (node instanceof Map) {
      yield* iteratePathTree(node);
    } else {
      yield node;
    }
  }
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

  const directoryCheckedStates = useMemo(
    () => (props.selection == null ? null : calculateTreeSelectionStates(tree, props.selection)),
    [tree, props.selection],
  );

  const [collapsed, setCollapsed] = useState(new Set());

  function renderTree(tree: PathTree<UIChangedFile>, accumulatedPath = '') {
    return (
      <div className="file-tree">
        {[...tree.entries()].map(([folder, inner]) => {
          const folderKey = `${accumulatedPath}/${folder}`;
          const isCollapsed = collapsed.has(folderKey);

          let content;
          if (inner instanceof Map) {
            const checkedState = directoryCheckedStates?.get(folderKey);
            content = (
              <>
                <FileTreeFolderHeader
                  isCollapsed={isCollapsed}
                  checkedState={checkedState}
                  toggleChecked={
                    rest.selection == null
                      ? undefined
                      : checked => {
                          const paths = mapIterable(iteratePathTree(inner), file => file.path);
                          if (checked) {
                            rest.selection?.select(...paths);
                          } else {
                            rest.selection?.deselect(...paths);
                          }
                        }
                  }
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
            );
          } else {
            content = <File key={inner.path} {...rest} file={inner} />;
          }

          return (
            <div className="file-tree-level" key={folderKey}>
              {content}
            </div>
          );
        })}
      </div>
    );
  }

  return renderTree(tree);
}
