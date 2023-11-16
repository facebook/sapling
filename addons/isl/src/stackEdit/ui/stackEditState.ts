/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';
import type {CommitState} from '../commitStackState';
import type {RecordOf} from 'immutable';
import type {SetterOrUpdater} from 'recoil';
import type {ExportStack} from 'shared/types/stack';

import {globalRecoil} from '../../AccessGlobalRecoil';
import clientToServerAPI from '../../ClientToServerAPI';
import {latestCommitMessageFieldsWithEdits} from '../../CommitInfoView/CommitInfoState';
import {
  commitMessageFieldsSchema,
  commitMessageFieldsToString,
} from '../../CommitInfoView/CommitMessageFields';
import {getTracker} from '../../analytics/globalTracker';
import {CommitStackState} from '../../stackEdit/commitStackState';
import {assert} from '../../utils';
import {List, Record} from 'immutable';
import {atom, DefaultValue, selector, useRecoilState} from 'recoil';
import {unwrap} from 'shared/utils';

type StackStateWithOperationProps = {
  op: StackEditOpDescription;
  state: CommitStackState;
  splitRange: SplitRangeRecord;
};

type Intention = 'general' | 'split';

/** Description of a stack edit operation. Used for display purpose. */
export type StackEditOpDescription =
  | {
      name: 'move';
      offset: number;
      /** Count of dependencies excluding self. */
      depCount?: number;
      commit: CommitState;
    }
  | {
      name: 'drop';
      commit: CommitState;
    }
  | {
      name: 'fold';
      commit: CommitState;
    }
  | {name: 'import'}
  | {name: 'insertBlankCommit'}
  | {name: 'fileStack'; fileDesc: string}
  | {name: 'split'; path: string}
  | {name: 'metaedit'; commit: CommitState};

type SplitRangeProps = {
  startKey: string;
  endKey: string;
};
export const SplitRangeRecord = Record<SplitRangeProps>({startKey: '', endKey: ''});
export type SplitRangeRecord = RecordOf<SplitRangeProps>;

const StackStateWithOperation = Record<StackStateWithOperationProps>({
  op: {name: 'import'},
  state: new CommitStackState([]),
  splitRange: SplitRangeRecord(),
});
type StackStateWithOperation = RecordOf<StackStateWithOperationProps>;

/** History of multiple states for undo/redo support. */
type HistoryProps = {
  history: List<StackStateWithOperation>;
  currentIndex: number;
};

const HistoryRecord = Record<HistoryProps>({
  history: List(),
  currentIndex: 0,
});
type HistoryRecord = RecordOf<HistoryProps>;

class History extends HistoryRecord {
  get current(): StackStateWithOperation {
    return unwrap(this.history.get(this.currentIndex));
  }

  push(
    state: CommitStackState,
    op: StackEditOpDescription,
    splitRange?: SplitRangeRecord,
  ): History {
    const newSplitRange = splitRange ?? this.current.splitRange;
    const newHistory = this.history
      .slice(0, this.currentIndex + 1)
      .push(StackStateWithOperation({op, state, splitRange: newSplitRange}));
    return new History({
      history: newHistory,
      currentIndex: newHistory.size - 1,
    });
  }

  setSplitRange(range: SplitRangeRecord): History {
    const newHistory = this.history.set(this.currentIndex, this.current.set('splitRange', range));
    return new History({
      history: newHistory,
      currentIndex: newHistory.size - 1,
    });
  }

  canUndo(): boolean {
    return this.currentIndex > 0;
  }

  canRedo(): boolean {
    return this.currentIndex + 1 < this.history.size;
  }

  undoOperationDescription(): StackEditOpDescription | undefined {
    return this.canUndo() ? this.history.get(this.currentIndex)?.op : undefined;
  }

  redoOperationDescription(): StackEditOpDescription | undefined {
    return this.canRedo() ? this.history.get(this.currentIndex + 1)?.op : undefined;
  }

  undo(): History {
    return this.canUndo() ? this.set('currentIndex', this.currentIndex - 1) : this;
  }

  redo(): History {
    return this.canRedo() ? this.set('currentIndex', this.currentIndex + 1) : this;
  }
}

/** State related to stack editing UI. */
type StackEditState = {
  /**
   * Commit hashes being edited.
   * Empty means no editing is requested.
   *
   * Changing this to a non-empty value triggers `exportStack`
   * message to the server.
   */
  hashes: Set<Hash>;

  /** Intention of the stack editing. */
  intention: Intention;

  /**
   * The (mutable) main history of stack states.
   */
  history: Loading<History>;
};

/** Lightweight recoil Loadable alternative that is not coupled with Promise. */
export type Loading<T> =
  | {
      state: 'loading';
      exportedStack:
        | ExportStack /* Got the exported stack. Analyzing. */
        | undefined /* Haven't got the exported stack. */;
    }
  | {state: 'hasValue'; value: T}
  | {state: 'hasError'; error: string};

// This is private so we can maintain state consistency
// (ex. stack and requested hashes cannot be out of sync)
// via selectors.
const stackEditState = atom<StackEditState>({
  key: 'stackEditState',
  default: {
    hashes: new Set<Hash>(),
    intention: 'general',
    history: {state: 'loading', exportedStack: undefined},
  },
  effects: [
    // Subscribe to server exportedStack events.
    ({setSelf}) => {
      const disposable = clientToServerAPI.onMessageOfType('exportedStack', event => {
        setSelf(prev => {
          const {hashes, intention} =
            prev instanceof DefaultValue
              ? {hashes: new Set<Hash>(), intention: 'general' as Intention}
              : prev;
          const revs = getRevs(hashes);
          if (revs !== event.revs) {
            // Wrong stack. Ignore it.
            return prev;
          }
          if (event.error != null) {
            return {hashes, intention, history: {state: 'hasError', error: event.error}};
          } else {
            return {
              hashes,
              intention,
              history: {state: 'loading', exportedStack: rewriteCommitMessagesInStack(event.stack)},
            };
          }
        });
      });
      return () => disposable.dispose();
    },
    // Kick off stack analysis on receiving an exported stack.
    ({setSelf, onSet}) => {
      onSet(newValue => {
        const {hashes, intention, history} = newValue;
        if (hashes.size > 0 && history.state === 'loading' && history.exportedStack !== undefined) {
          try {
            const stack = new CommitStackState(history.exportedStack).buildFileStacks();
            const historyValue = new History({
              history: List([StackStateWithOperation({state: stack})]),
              currentIndex: 0,
            });
            currentMetrics = {
              commits: hashes.size,
              fileStacks: stack.fileStacks.size,
              fileStackRevs: stack.fileStacks.reduce((acc, f) => acc + f.source.revLength, 0),
            };
            currentMetricsStartTime = Date.now();
            setSelf({hashes, intention, history: {state: 'hasValue', value: historyValue}});
          } catch (err) {
            const msg = `Cannot construct stack ${err}`;
            setSelf({hashes, intention, history: {state: 'hasError', error: msg}});
          }
        }
      });
    },
  ],
});

/**
 * Update commits messages in an exported stack to include:
 * 1. Any local edits the user has pending (these have already been confirmed by a modal at this point)
 * 2. Any remote message changes from the server (which allows the titles in the edit stack UI to be up to date)
 */
function rewriteCommitMessagesInStack(stack: ExportStack): ExportStack {
  const schema = globalRecoil().getLoadable(commitMessageFieldsSchema).valueMaybe();
  return stack.map(c => {
    let text = c.text;
    if (schema) {
      const editedMessage = globalRecoil()
        .getLoadable(latestCommitMessageFieldsWithEdits(c.node))
        .valueMaybe();
      if (editedMessage != null) {
        text = commitMessageFieldsToString(schema, editedMessage);
      }
    }
    return {...c, text};
  });
}

/**
 * Commit hashes being stack edited for general purpose.
 * Setting to a non-empty value triggers server-side loading.
 */
export const editingStackIntentionHashes = selector<[Intention, Set<Hash>]>({
  key: 'editingStackIntentionHashes',
  get: ({get}) => {
    const state = get(stackEditState);
    return [state.intention, state.hashes];
  },
  set: ({set}, newValue) => {
    const [intention, hashes] =
      newValue instanceof DefaultValue ? ['general' as Intention, new Set<Hash>()] : newValue;
    if (hashes.size > 0) {
      const revs = getRevs(hashes);
      clientToServerAPI.postMessage({type: 'exportStack', revs});
    }
    set(stackEditState, {
      hashes,
      intention,
      history: {state: 'loading', exportedStack: undefined},
    });
  },
});

/**
 * State for check whether the stack is loaded or not.
 * Use `useStackEditState` if you want to read or edit the stack.
 *
 * This is not `Loading<CommitStackState>` so `hasValue`
 * states do not trigger re-render.
 */
export const loadingStackState = selector<Loading<null>>({
  key: 'loadingStackState',
  get: ({get}) => {
    const history = get(stackEditState).history;
    if (history.state === 'hasValue') {
      return hasValueState;
    } else {
      return history;
    }
  },
});
const hasValueState: Loading<null> = {state: 'hasValue', value: null};

/** APIs exposed via useStackEditState() */
class UseStackEditState {
  state: StackEditState;
  setState: SetterOrUpdater<StackEditState>;

  // derived properties.
  private history: History;

  constructor(state: StackEditState, setState: SetterOrUpdater<StackEditState>) {
    this.state = state;
    this.setState = setState;
    assert(
      state.history.state === 'hasValue',
      'useStackEditState only works when the stack is loaded',
    );
    this.history = state.history.value;
  }

  get commitStack(): CommitStackState {
    return this.history.current.state;
  }

  get splitRange(): SplitRangeRecord {
    return this.history.current.splitRange;
  }

  get intention(): Intention {
    return this.state.intention;
  }

  setSplitRange(range: SplitRangeRecord | string) {
    const splitRange =
      typeof range === 'string'
        ? SplitRangeRecord({
            startKey: range,
            endKey: range,
          })
        : range;
    const newHistory = this.history.setSplitRange(splitRange);
    this.setHistory(newHistory);
  }

  push(commitStack: CommitStackState, op: StackEditOpDescription, splitRange?: SplitRangeRecord) {
    if (commitStack.originalStack !== this.commitStack.originalStack) {
      // Wrong stack. Discard.
      return;
    }
    const newHistory = this.history.push(commitStack, op, splitRange);
    this.setHistory(newHistory);
  }

  canUndo(): boolean {
    return this.history.canUndo();
  }

  canRedo(): boolean {
    return this.history.canRedo();
  }

  undo() {
    this.setHistory(this.history.undo());
  }

  undoOperationDescription(): StackEditOpDescription | undefined {
    return this.history.undoOperationDescription();
  }

  redoOperationDescription(): StackEditOpDescription | undefined {
    return this.history.redoOperationDescription();
  }

  redo() {
    this.setHistory(this.history.redo());
  }

  private setHistory(newHistory: History) {
    const {hashes, intention} = this.state;
    this.setState({
      hashes,
      intention,
      history: {state: 'hasValue', value: newHistory},
    });
  }
}

// Only export the type, not the constructor.
export type {UseStackEditState};

/**
 * Get the stack edit state. The stack must be loaded already, that is,
 * `loadingStackState.state` is `hasValue`. This is the main state for
 * reading and updating the `CommitStackState`.
 */
// This is not a recoil selector for flexibility.
// See https://github.com/facebookexperimental/Recoil/issues/673
export function useStackEditState() {
  const [state, setState] = useRecoilState(stackEditState);
  return new UseStackEditState(state, setState);
}

/** Get revset expression for requested hashes. */
function getRevs(hashes: Set<Hash>): string {
  return [...hashes].join('|');
}

type StackEditMetrics = {
  // Managed by this file.
  commits: number;
  fileStacks: number;
  fileStackRevs: number;
  // Maintained by UI, via 'bumpStackEditMetric'.
  undo?: number;
  redo?: number;
  fold?: number;
  drop?: number;
  moveUpDown?: number;
  moveDnD?: number;
  fileStackEdit?: number;
  splitMoveFile?: number;
  splitMoveLine?: number;
  splitInsertBlank?: number;
  splitChangeRange?: number;
};

// Not atoms. They do not trigger re-render.
let currentMetrics: StackEditMetrics = {commits: 0, fileStackRevs: 0, fileStacks: 0};
let currentMetricsStartTime = 0;

export function bumpStackEditMetric(key: keyof StackEditMetrics) {
  currentMetrics[key] = (currentMetrics[key] ?? 0) + 1;
}

export function sendStackEditMetrics(save = true) {
  const tracker = getTracker();
  const duration = Date.now() - currentMetricsStartTime;
  tracker?.track('StackEditMetrics', {duration, extras: {...currentMetrics, save}});
}
