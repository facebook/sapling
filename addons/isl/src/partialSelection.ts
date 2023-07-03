/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath} from './types';

import {ChunkSelectState} from './stackEdit/chunkSelectState';
import {assert} from './utils';
import Immutable from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';

type SingleFileSelection =
  | false /* not selected */
  | true /* selected, default */
  | ChunkSelectState /* maybe partially selected */;

type PartialSelectionProps = {
  /** Explicitly set selection. */
  fileMap: Immutable.Map<RepoRelativePath, SingleFileSelection>;
  /** For files not in fileMap, whether they are selected or not. */
  selectByDefault: boolean;
};
const PartialSelectionRecord = Immutable.Record<PartialSelectionProps>({
  fileMap: Immutable.Map(),
  selectByDefault: true,
});
type PartialSelectionRecord = Immutable.RecordOf<PartialSelectionProps>;

/**
 * Selection of partial changes made by a commit.
 *
 * Intended to be useful for both concrete commits and the `wdir()` virtual commit.
 * This class does not handle the differences between `wdir()` and concrete commits,
 * like how to load the file content, and how to get the list of changed files.
 * Those differences are handled at a higher level.
 */
export class PartialSelection extends SelfUpdate<PartialSelectionRecord> {
  constructor(record: PartialSelectionRecord) {
    super(record);
  }

  /** Empty selection. */
  static empty(props: {selectByDefault?: boolean}): PartialSelection {
    return new PartialSelection(PartialSelectionRecord(props));
  }

  /** Explicitly select a file. */
  select(path: RepoRelativePath): PartialSelection {
    return new PartialSelection(this.inner.setIn(['fileMap', path], true));
  }

  /** Explicitly deselect a file. */
  deselect(path: RepoRelativePath): PartialSelection {
    return new PartialSelection(this.inner.setIn(['fileMap', path], false));
  }

  /** Reset to the "default" state. Useful for commit/amend. */
  clear(): PartialSelection {
    return new PartialSelection(this.inner.set('fileMap', Immutable.Map()));
  }

  /** Start chunk selection for the given file. */
  startChunkSelect(
    path: RepoRelativePath,
    a: string,
    b: string,
    selected: boolean | string,
  ): PartialSelection {
    const chunkState = ChunkSelectState.fromText(a, b, selected);
    return new PartialSelection(this.inner.setIn(['fileMap', path], chunkState));
  }

  /** Edit chunk selection for a file. */
  editChunkSelect(
    path: RepoRelativePath,
    newValue: ((chunkState: ChunkSelectState) => ChunkSelectState) | ChunkSelectState,
  ): PartialSelection {
    const chunkState = this.inner.fileMap.get(path);
    assert(
      chunkState instanceof ChunkSelectState,
      'PartialSelection.editChunkSelect() called without startChunkEdit',
    );
    const newChunkState = typeof newValue === 'function' ? newValue(chunkState) : newValue;
    return new PartialSelection(this.inner.setIn(['fileMap', path], newChunkState));
  }

  getSelection(path: RepoRelativePath): SingleFileSelection {
    const record = this.inner;
    return record.fileMap.get(path) ?? record.selectByDefault;
  }

  /**
   * Return true if a file is selected, false if deselected,
   * or a string with the edited content.
   * Even if the file is being chunk edited, this function might
   * still return true or false.
   */
  getSimplifiedSelection(path: RepoRelativePath): boolean | string {
    const selected = this.getSelection(path);
    if (selected === true || selected === false) {
      return selected;
    }
    const chunkState: ChunkSelectState = selected;
    const text = chunkState.getSelectedText();
    if (text === chunkState.a) {
      return false;
    }
    if (text === chunkState.b) {
      return true;
    }
    return text;
  }

  isFullyOrPartiallySelected(path: RepoRelativePath): boolean {
    return this.getSimplifiedSelection(path) !== false;
  }

  isPartiallySelected(path: RepoRelativePath): boolean {
    return typeof this.getSimplifiedSelection(path) !== 'boolean';
  }

  isFullySelected(path: RepoRelativePath): boolean {
    return this.getSimplifiedSelection(path) === true;
  }

  isDeselected(path: RepoRelativePath): boolean {
    return this.getSimplifiedSelection(path) === false;
  }

  isEverythingSelected(getAllPaths: () => Array<RepoRelativePath>): boolean {
    const record = this.inner;
    const paths = record.selectByDefault ? record.fileMap.keySeq() : getAllPaths();
    return paths.every(p => this.getSimplifiedSelection(p) === true);
  }

  isNothingSelected(getAllPaths: () => Array<RepoRelativePath>): boolean {
    const record = this.inner;
    const paths = record.selectByDefault ? getAllPaths() : record.fileMap.keySeq();
    return paths.every(p => this.getSimplifiedSelection(p) === false);
  }

  /** If any file is partially selected. */
  hasChunkSelection(): boolean {
    return this.inner.fileMap
      .keySeq()
      .some(p => typeof this.getSimplifiedSelection(p) !== 'boolean');
  }
}
