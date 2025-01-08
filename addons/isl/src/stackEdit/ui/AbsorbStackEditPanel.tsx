/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../../dag/dag';
import type {DagCommitInfo} from '../../dag/dagCommitInfo';
import type {AbsorbEdit, AbsorbEditId} from '../absorb';
import type {CommitStackState, FileStackIndex} from '../commitStackState';
import type {Map as ImMap} from 'immutable';

import {FileHeader, IconType} from '../../ComparisonView/SplitDiffView/SplitDiffFileHeader';
import {RenderDag} from '../../RenderDag';
import {YOU_ARE_HERE_VIRTUAL_COMMIT} from '../../dag/virtualCommit';
import {calculateDagFromStack} from '../stackDag';
import {useStackEditState} from './stackEditState';
import * as stylex from '@stylexjs/stylex';
import {nullthrows} from 'shared/utils';

const styles = stylex.create({
  absorbEditSingleChunk: {
    border: '1px solid var(--tooltip-border)',
    // The negative margins match <FileHeader />.
    marginLeft: -1,
    marginRight: -1,
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
});

export function AbsorbStackEditPanel() {
  const stackEdit = useStackEditState();
  const stack = stackEdit.commitStack;
  const dag = calculateDagFromStack(stack);
  const subset = relevantSubset(stack, dag);
  return (
    <RenderDag
      dag={dag}
      renderCommit={renderCommit}
      renderCommitExtras={renderCommitExtras}
      subset={subset}
    />
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

function SingleAbsorbEdit(props: {edit: AbsorbEdit}) {
  const {edit} = props;
  return (
    <div {...stylex.props(styles.absorbEditSingleChunk)}>
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
