/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useState} from 'react';

import './copyFromSelectedColumn.css';

function copyFromSelectedColumn(e: React.ClipboardEvent<HTMLTableElement>) {
  const clipboard = e.clipboardData;
  const text = getSelectedColumnText();
  if (!text) {
    return;
  }
  e.preventDefault();
  clipboard.setData('text/plain', text);
}

function getSelectedColumnText(numColumns = 4): string | undefined {
  const sel = window.getSelection();
  if (sel == null) {
    return;
  }
  const range = sel.getRangeAt(0);
  const doc = range.cloneContents();
  const nodes = doc.querySelectorAll('tr');

  if (nodes.length === 0) {
    return doc.textContent ?? '';
  }

  let text = '';

  // We know how many columns are in the table, so each tr should contain ${numColumns} tds.
  // But the selection may not be aligned at the start or end:
  //    start drag here
  //           v
  //           CC DDD
  //  AAA BBB CCC DDD
  //  AAA BBB CCC DDD
  //  AAA B
  //      ^
  //  end drag here

  // We can compute what side we started dragging from by comparing
  // the first row's length to the expected length of 4:
  const idx = numColumns - nodes[0].children.length;

  nodes.forEach((tr, i) => {
    // The first row will be missing columns before the dragged point,
    // so idx will always be 0 for that column.
    // The last row also has the wrong number of columns, but idx is still correct
    // as long as it's within bounds.
    const newIdx = i === 0 ? 0 : idx;
    if (newIdx >= 0) {
      const td = tr.cells[newIdx];
      if (td) {
        text += (i > 0 ? '\n' : '') + td.textContent;
      }
    }
  });

  return text;
}

/**
 * When using a <table> to render columns of text, add support
 * for selecting ( & copying) within any one column.
 * Returns props to be forwarded to the table.
 *
 * Note: you need to add data-column={3} prop (replace 3 with appropriate column index) to your <td>s to get the
 * selection to only appear in one column.
 */
export function useTableColumnSelection(): React.TableHTMLAttributes<HTMLTableElement> {
  const [selectedColumn, setSelectedColumn] = useState(0);

  return {
    onCopy: copyFromSelectedColumn,
    className: `single-column-selection-table selected-column-${selectedColumn}`,
    onMouseDown: e => {
      const td = findParent('TD', e.target as HTMLElement) as HTMLTableCellElement;
      if (td) {
        const col = td.cellIndex;
        setSelectedColumn(col);
      }
    },
  };
}

function findParent(type: string, start: HTMLElement): HTMLElement {
  let el = start;
  while (el.tagName !== type) {
    if (el.parentElement == null) {
      return el;
    }
    el = el.parentElement;
  }
  return el;
}
