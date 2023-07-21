/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RangeInfo} from './TextEditable';
import type {ChunkSelectState, LineRegion, SelectLine} from './stackEdit/chunkSelectState';

import {TextEditable} from './TextEditable';
import {T, t} from './i18n';
import {Set as ImSet} from 'immutable';
import {useRef, useState} from 'react';
import {notEmpty} from 'shared/utils';

import './PartialFileSelection.css';

type Props = {
  chunkSelection: ChunkSelectState;
  setChunkSelection: (state: ChunkSelectState) => void;
};

export function PartialFileSelection(props: Props) {
  // States for context line expansion.
  const [expandedALines, setExpandedALines] = useState<ImSet<number>>(ImSet);
  const [currentCaretLine, setCurrentSelLine] = useState<number>(-1);

  // State for range selection.
  const lastLine = useRef<SelectLine | null>(null);

  const lineRegions = props.chunkSelection.getLineRegions({
    expandedALines,
    expandedSelLine: currentCaretLine,
  });

  // Toggle selection of a line or a region.
  const handlePointerDown = (
    line: SelectLine,
    region: LineRegion | null,
    e: React.PointerEvent<HTMLDivElement>,
  ) => {
    if (e.isPrimary && line.selected !== null) {
      const selected = !line.selected;
      const lineSelects: Array<[number, boolean]> = [];
      if (region) {
        region.lines.forEach(line => {
          lineSelects.push([line.rawIndex, selected]);
        });
      } else {
        lineSelects.push([line.rawIndex, selected]);
      }
      const newSelection = props.chunkSelection.setSelectedLines(lineSelects);
      lastLine.current = line;
      props.setChunkSelection(newSelection);
    }
  };

  // Toogle selection of a single line.
  const handlePointerEnter = (line: SelectLine, e: React.PointerEvent<HTMLDivElement>) => {
    if (e.buttons === 1 && line.selected !== null && lastLine.current?.rawIndex !== line.rawIndex) {
      const newSelection = props.chunkSelection.setSelectedLines([[line.rawIndex, !line.selected]]);
      lastLine.current = line;
      props.setChunkSelection(newSelection);
    }
  };

  // Needed by TextEditable. Ranges of text on the right side.
  const rangeInfos: RangeInfo[] = [];
  let start = 0;

  // Render the rows.
  const lineAContent: JSX.Element[] = [];
  const lineBContent: JSX.Element[] = [];
  const lineANumber: JSX.Element[] = [];
  const lineBNumber: JSX.Element[] = [];

  const insertContextLines = (lines: Readonly<SelectLine[]>) => {
    // Capture `skipStart` and `i` in a local variable inside loop body.
    const handleExpand = () => {
      // Only the "unchanged" lines need expansion.
      // We use line numbers on the "a" side, which remains "stable" regardless of editing.
      const newLines = lines.map(l => l.aLine).filter(notEmpty);
      const newSet = expandedALines.union(newLines);
      setExpandedALines(newSet);
    };
    const key = lines[0].rawIndex;
    const contextLineContent = (
      <div
        key={key}
        className="line line-context"
        title={t('Click to expand lines.')}
        onClick={handleExpand}
      />
    );
    lineAContent.push(contextLineContent);
    lineBContent.push(contextLineContent);
    const contextLineNumber = <div key={key} className="line-number line-context" />;
    lineANumber.push(contextLineNumber);
    lineBNumber.push(contextLineNumber);
  };

  lineRegions.forEach(region => {
    if (region.collapsed) {
      // Draw "~~~" between chunks.
      insertContextLines(region.lines);
    }

    region.lines.forEach(line => {
      let dataRangeId = undefined;
      // Provide `RangeInfo` for editing, if the line exists in the selection version.
      if (line.selLine !== null) {
        const end = start + line.data.length;
        dataRangeId = rangeInfos.length;
        rangeInfos.push({start, end});
        start = end;
      }

      if (region.collapsed) {
        return;
      }

      let rowClass =
        line.selected === null ? 'unselectable' : line.selected ? 'selected ' : 'deselected';

      // Draw the actual line and line numbers.
      let aLineData = line.data;
      const bLineData = dataRangeId === undefined ? '\n' : line.data;
      if (line.sign === '+') {
        if (line.selected) {
          aLineData = '\n';
        }
        rowClass += ' line-add';
      } else if (line.sign === '!+') {
        aLineData = '\n';
        rowClass += ' line-force-add';
      } else if (line.sign === '-') {
        rowClass += ' line-del';
      } else if (line.sign === '!-') {
        rowClass += ' line-force-del';
      }

      const lineNumberProps = {
        onPointerDown: handlePointerDown.bind(null, line, null),
        onPointerEnter: handlePointerEnter.bind(null, line),
        title:
          line.selected == null
            ? undefined
            : t('Click to toggle line selection. Drag for range selection.'),
      };

      const lineContentProps = {
        onPointerDown: handlePointerDown.bind(null, line, region),
        title: line.selected == null ? undefined : t('Click to toggle chunk selection.'),
      };

      const key = line.rawIndex;
      lineAContent.push(
        <div key={key} className={`line line-a ${rowClass}`} {...lineContentProps}>
          {aLineData}
        </div>,
      );
      lineBContent.push(
        <div key={key} className={`line line-b ${rowClass}`} data-range-id={dataRangeId}>
          {bLineData}
        </div>,
      );

      lineANumber.push(
        <div key={key} className={`line-number line-a ${rowClass}`} {...lineNumberProps}>
          {line.aLine ?? '\n'}
        </div>,
      );
      lineBNumber.push(
        <div key={key} className={`line-number line-b ${rowClass}`} {...lineNumberProps}>
          {line.selLine ?? '\n'}
        </div>,
      );
    });
  });

  const textValue = props.chunkSelection.getSelectedText();
  const handleTextChange = (text: string) => {
    const newChunkSelect = props.chunkSelection.setSelectedText(text);
    props.setChunkSelection(newChunkSelect);
  };
  const handleSelChange = (start: number, end: number) => {
    // Expand the line of the cursor. But do not expand on range selection (ex. Ctrl+A).
    if (start === end) {
      let selLine = countLines(textValue.substring(0, start));
      if (start == textValue.length && textValue.endsWith('\n')) {
        selLine -= 1;
      }
      setCurrentSelLine(selLine);
    }
  };

  return (
    <div className="partial-file-selection-width-min-content">
      <div className="partial-file-selection-scroll-y">
        <div className="partial-file-selection">
          <div className="partial-file-selection-scroll-x">
            <pre className="column-a">{lineAContent}</pre>
          </div>
          <pre className="column-a-number">{lineANumber}</pre>
          <pre className="column-b-number">{lineBNumber}</pre>
          <div className="partial-file-selection-scroll-x">
            <TextEditable
              value={textValue}
              rangeInfos={rangeInfos}
              onTextChange={handleTextChange}
              onSelectChange={handleSelChange}>
              <pre className="column-b">{lineBContent}</pre>
            </TextEditable>
          </div>
        </div>
      </div>
      <div className="partial-file-selection-tip">
        <T>Click lines on the left side, or line numbers to toggle selection. </T>
        <T>The right side shows the selection result.</T>
        <br />
        <T>
          You can also edit the code on the right side. Edit affects partial selection. Files on
          disk are not affected.
        </T>
      </div>
    </div>
  );
}

function countLines(text: string): number {
  let result = 1;
  for (const ch of text) {
    if (ch === '\n') {
      result++;
    }
  }
  return result;
}
