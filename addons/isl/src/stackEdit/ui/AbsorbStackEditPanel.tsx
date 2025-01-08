/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from '../../DragHandle';
import type {RenderGlyphResult} from '../../RenderDag';
import type {Dag} from '../../dag/dag';
import type {DagCommitInfo} from '../../dag/dagCommitInfo';
import type {AbsorbEdit, AbsorbEditId} from '../absorb';
import type {CommitStackState, FileStackIndex, Rev} from '../commitStackState';
import type {Map as ImMap} from 'immutable';

import {FileHeader, IconType} from '../../ComparisonView/SplitDiffView/SplitDiffFileHeader';
import {DragHandle} from '../../DragHandle';
import {DraggingOverlay} from '../../DraggingOverlay';
import {defaultRenderGlyph, RenderDag} from '../../RenderDag';
import {YOU_ARE_HERE_VIRTUAL_COMMIT} from '../../dag/virtualCommit';
import {t, T} from '../../i18n';
import {readAtom, writeAtom} from '../../jotaiUtils';
import {calculateDagFromStack} from '../stackDag';
import {stackEditStack, useStackEditState} from './stackEditState';
import * as stylex from '@stylexjs/stylex';
import {Column, Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {atom, useAtomValue} from 'jotai';
import {nullthrows} from 'shared/utils';

const styles = stylex.create({
  absorbEditSingleChunk: {
    border: '1px solid var(--tooltip-border)',
    // The negative margins match <FileHeader />.
    marginLeft: -1,
    marginRight: -1,
    display: 'flex',
  },
  inDraggingOverlay: {
    border: 'none',
  },
  beingDragged: {
    opacity: 0.5,
  },
  dragHandlerWrapper: {
    width: 'fit-content',
    display: 'flex',
    alignItems: 'center',
    padding: '0 var(--pad)',
    backgroundColor: {
      ':hover': 'var(--tooltip-background)',
    },
  },
  candidateDropTarget: {
    backgroundColor: 'var(--tooltip-background)',
  },
  absorbEditCode: {
    borderCollapse: 'collapse',
    wordBreak: 'break-all',
    whiteSpace: 'pre-wrap',
    fontFamily: 'var(--monospace-fontFamily)',
    fontSize: 'var(--editor-font-size)',
  },
  absorbEditPathTitle: {
    padding: 'var(--halfpad) var(--pad)',
  },
  addLine: {
    backgroundColor: 'var(--diffEditor-insertedLineBackground)',
  },
  delLine: {
    backgroundColor: 'var(--diffEditor-removedLineBackground)',
  },
  lineContentCell: {
    minWidth: 300,
  },
  lineContentText: {
    marginLeft: 'var(--pad)',
  },
  lineNumber: {
    textAlign: 'right',
    whiteSpace: 'nowrap',
    minWidth: 50,
  },
  lineNumberSpan: {
    marginRight: 'var(--pad)',
  },
  addLineNumber: {
    backgroundColor: 'var(--diffEditor-insertedLineHighlightBackground)',
  },
  delLineNumber: {
    backgroundColor: 'var(--diffEditor-removedLineHighlightBackground)',
  },
  inselectable: {
    userSelect: 'none',
  },
  commitTitle: {
    padding: 'var(--halfpad) var(--pad)',
  },
  instruction: {
    padding: 'var(--halfpad) var(--pad)',
  },
});

/** The `AbsorbEdit` that is currently being dragged. */
const draggingAbsorbEdit = atom<AbsorbEdit | null>(null);
const draggingHint = atom<string | null>(null);
const onDragRef: {current: null | DragHandler} = {current: null};

export function AbsorbStackEditPanel() {
  const stackEdit = useStackEditState();
  const stack = stackEdit.commitStack;
  const dag = calculateDagFromStack(stack);
  const subset = relevantSubset(stack, dag);
  return (
    <>
      <Column>
        <div {...stylex.props(styles.instruction)}>
          <Row>
            <Icon icon="info" />
            <div>
              <T>Drag a diff chunk to a commit to amend the diff chunk into the commit.</T>
              <br />
              <T>Diff chunks under "You are here" will be left in the working copy.</T>
              <br />
              <T>Only commits that modify related files are shown.</T>
            </div>
          </Row>
        </div>
        <RenderDag
          className="absorb-dag"
          dag={dag}
          renderCommit={renderCommit}
          renderCommitExtras={renderCommitExtras}
          renderGlyph={RenderGlyph}
          subset={subset}
          style={{
            /* make it "containing block" so findDragDestinationCommitKey works */
            position: 'relative',
          }}
        />
      </Column>
      <AbsorbDraggingOverlay />
    </>
  );
}

const candidateDropTargetRevs = atom<readonly Rev[] | undefined>(get => {
  const edit = get(draggingAbsorbEdit);
  const stack = get(stackEditStack);
  if (edit == null || stack == null) {
    return undefined;
  }
  return stack.getAbsorbCommitRevs(nullthrows(edit.fileStackIndex), edit.absorbEditId)
    .candidateRevs;
});

function RenderGlyph(info: DagCommitInfo): RenderGlyphResult {
  const revs = useAtomValue(candidateDropTargetRevs);
  const rev = info.stackRev;
  const [kind, inner] = defaultRenderGlyph(info);
  let newInner = inner;
  if (kind === 'inside-tile' && rev != null && revs?.includes(rev)) {
    // This is a candidate drop target. Wrap in a SVG circle.
    const circle = (
      <circle cx={0} cy={0} r={8} fill="transparent" stroke="var(--focus-border)" strokeWidth={4} />
    );
    newInner = (
      <>
        {circle}
        {inner}
      </>
    );
  }
  return [kind, newInner];
}

function AbsorbDraggingOverlay() {
  const absorbEdit = useAtomValue(draggingAbsorbEdit);
  const hint = useAtomValue(draggingHint);
  return (
    <DraggingOverlay onDragRef={onDragRef} hint={hint}>
      {absorbEdit && <SingleAbsorbEdit edit={absorbEdit} inDraggingOverlay={true} />}
    </DraggingOverlay>
  );
}

/**
 * Subset of `dag` to render in an absorb UI. It skips draft commits that are
 * not absorb destinations. For example, A01..A50, a 50-commit stack, absorbing
 * `x.txt` change. There are only 3 commits that touch `x.txt`, so the absorb
 * destination only includes those 3 commits.
 */
function relevantSubset(stack: CommitStackState, dag: Dag) {
  const revs = stack.getAllAbsorbCandidateCommitRevs();
  const keys = [...revs].map(rev => nullthrows(stack.get(rev)?.key));
  // Also include the (base) public commit and the `wdir()` virtual commit.
  keys.push(YOU_ARE_HERE_VIRTUAL_COMMIT.hash);
  return dag.present(keys).union(dag.public_());
}

// NOTE: To avoid re-render, the "renderCommit" and "renderCommitExtras" functions
// need to be "static" instead of anonymous functions.
function renderCommit(info: DagCommitInfo) {
  // Just show the commit title for now.
  return <div {...stylex.props(styles.commitTitle)}>{info.title}</div>;
}

function renderCommitExtras(info: DagCommitInfo) {
  return <AbsorbDagCommitExtras info={info} />;
}

/**
 * Scan the absorb dag DOM and extract [data-reorder-id], or the commit key,
 * from the dragging destination.
 */
function findDragDestinationCommitKey(y: number): string | undefined {
  const container = document.querySelector('.absorb-dag');
  if (container == null) {
    return undefined;
  }
  const containerY = container.getBoundingClientRect().y;
  const relativeY = y - containerY;
  let bestKey: string | undefined = undefined;
  let bestDelta: number = Infinity;
  for (const element of container.querySelectorAll('.render-dag-row-group')) {
    const divElement = element as HTMLDivElement;
    // use offSetTop instead of getBoundingClientRect() to avoid
    // being affected by ongoing animation.
    const y1 = divElement.offsetTop;
    const y2 = y1 + divElement.offsetHeight;
    const commitKey = divElement.getAttribute('data-reorder-id');
    const delta = Math.abs(relativeY - (y1 + y2) / 2);
    if (relativeY >= y1 && commitKey != null && delta < bestDelta) {
      bestKey = commitKey;
      bestDelta = delta;
    }
  }
  return bestKey;
}

/** Similar to `findDragDestinationCommitKey` but reports the rev. */
function findDragDestinationCommitRev(y: number, stack: CommitStackState): Rev | undefined {
  const key = findDragDestinationCommitKey(y);
  if (key == null) {
    return undefined;
  }
  // Convert key to rev.
  return stack.findRev(commit => commit.key === key);
}

/** Show file paths and diff chunks. */
function AbsorbDagCommitExtras(props: {info: DagCommitInfo}) {
  const {info} = props;
  const stackEdit = useStackEditState();
  const stack = stackEdit.commitStack;
  const rev = info.stackRev;
  if (rev == null) {
    return null;
  }

  const fileIdxToEdits = stack.absorbExtraByCommitRev(rev);
  if (fileIdxToEdits.isEmpty()) {
    return null;
  }

  return (
    <div>
      {fileIdxToEdits
        .map((edits, fileIdx) => (
          <AbsorbEditsForFile fileStackIndex={fileIdx} absorbEdits={edits} key={fileIdx} />
        ))
        .valueSeq()}
    </div>
  );
}

function AbsorbEditsForFile(props: {
  fileStackIndex: FileStackIndex;
  absorbEdits: ImMap<AbsorbEditId, AbsorbEdit>;
}) {
  const stackEdit = useStackEditState();
  const stack = stackEdit.commitStack;
  // Display a file path.
  // NOTE: In case the file is renamed, only the "before rename" path is shown.
  const path = stack.getFileStackPath(props.fileStackIndex, 0);
  return (
    <div>
      {path && <FileHeader path={path} iconType={IconType.Modified} />}
      {props.absorbEdits.map((edit, i) => <SingleAbsorbEdit edit={edit} key={i} />).valueSeq()}
    </div>
  );
}

function SingleAbsorbEdit(props: {edit: AbsorbEdit; inDraggingOverlay?: boolean}) {
  const {edit, inDraggingOverlay} = props;
  const isDragging = useAtomValue(draggingAbsorbEdit);
  const stackEdit = useStackEditState();

  const handleDrag = (x: number, y: number, isDragging: boolean) => {
    // Visual update.
    onDragRef.current?.(x, y, isDragging);
    // State update.
    if (isDragging) {
      // The 'stack' in the closure might be outdated. Read the latest.
      const stack = readAtom(stackEditStack);
      if (stack == null) {
        return;
      }
      const rev = findDragDestinationCommitRev(y, stack);
      const fileStackIndex = nullthrows(edit.fileStackIndex);
      const absorbEditId = edit.absorbEditId;
      if (
        rev != null &&
        rev !== stack?.getAbsorbCommitRevs(fileStackIndex, absorbEditId).selectedRev
      ) {
        const commit = nullthrows(stack.get(rev));
        let newStack = stack;
        try {
          newStack = stack.setAbsorbEditDestination(fileStackIndex, absorbEditId, rev);
          writeAtom(draggingHint, null);
        } catch {
          writeAtom(
            draggingHint,
            t(
              'Diff chunk can only be applied to commits that modify the file and has the context lines introduced earlier.',
            ),
          );
        }
        if (isDragging == null) {
          stackEdit.push(newStack, {name: 'absorbMove', commit});
        } else {
          stackEdit.replaceTopOperation(newStack, {name: 'absorbMove', commit});
        }
      }
    } else {
      // Ensure the hint is cleared when not dragging.
      // Otherwise the hint div might have unwanted side effects on interaction.
      writeAtom(draggingHint, null);
    }
    writeAtom(draggingAbsorbEdit, isDragging ? edit : null);
  };

  return (
    <div
      {...stylex.props(
        styles.absorbEditSingleChunk,
        inDraggingOverlay && styles.inDraggingOverlay,
        !inDraggingOverlay && isDragging === edit && styles.beingDragged,
      )}>
      <div {...stylex.props(styles.dragHandlerWrapper)}>
        <DragHandle onDrag={handleDrag}>
          <Icon icon="grabber" />
        </DragHandle>
      </div>
      <table {...stylex.props(styles.absorbEditCode)} border={0} cellPadding={0} cellSpacing={0}>
        <colgroup>
          <col width={50} />
          <col width="100%" />
        </colgroup>
        <tbody>
          {edit.oldLines.map((l, i) => (
            <DiffLine key={i} num={edit.oldStart + i} text={l} sign="-" />
          ))}
          {edit.newLines.map((l, i) => (
            <DiffLine key={i} num={edit.newStart + i} text={l} sign="+" />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DiffLine(props: {num: number; text: string; sign: '+' | '-'}) {
  const {num, text, sign} = props;
  return (
    <tr key={`${sign}${num}`}>
      <td
        {...stylex.props(
          sign === '+' ? styles.addLineNumber : styles.delLineNumber,
          styles.lineNumber,
          styles.inselectable,
        )}>
        <span {...stylex.props(styles.lineNumberSpan)}>{num}</span>
      </td>
      <td {...stylex.props(sign === '+' ? styles.addLine : styles.delLine, styles.lineContentCell)}>
        <span {...stylex.props(styles.lineContentText)}>{text}</span>
      </td>
    </tr>
  );
}
