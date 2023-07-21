/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ChunkSelectState} from '../chunkSelectState';

describe('ChunkSelectState', () => {
  const a = 'aa\nbb\ncc\ndd\n';
  const b = 'cc\ndd\nee\nff\n';

  describe('fromText()', () => {
    it('with none selected', () => {
      const state = ChunkSelectState.fromText(a, b, false);
      expect(renderLines(state)).toMatchObject([
        '[ ] -  1    aa',
        '[ ] -  2    bb',
        '       3  1 cc',
        '       4  2 dd',
        '[ ] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(a);
    });

    it('with all changes selected', () => {
      const state = ChunkSelectState.fromText(a, b, true);
      expect(renderLines(state)).toMatchObject([
        '[x] -  1    aa',
        '[x] -  2    bb',
        '       3  1 cc',
        '       4  2 dd',
        '[x] +     3 ee',
        '[x] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(b);
    });

    it('with free-form partial selection', () => {
      const text = 'aa\ncc\ndd\nee\n';
      const state = ChunkSelectState.fromText(a, b, text);
      expect(renderLines(state)).toMatchObject([
        '[ ] -  1    aa',
        '[x] -  2    bb',
        '       3  1 cc',
        '       4  2 dd',
        '[x] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(text);
    });

    it('with free-form extra deletions', () => {
      const text = '';
      const state = ChunkSelectState.fromText(a, b, text);
      expect(renderLines(state)).toMatchObject([
        '[x] -  1    aa',
        '[x] -  2    bb',
        '   !-  3  1 cc',
        '   !-  4  2 dd',
        '[ ] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(text);
    });

    it('with free-form extra insertions', () => {
      const text = 'aa\ncc\n<insertion>\ndd\nee\n';
      const state = ChunkSelectState.fromText(a, b, text);
      expect(renderLines(state)).toMatchObject([
        '[ ] -  1    aa',
        '[x] -  2    bb',
        '       3  1 cc',
        '   !+       <insertion>',
        '       4  2 dd',
        '[x] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(text);
    });

    it('sorts changes, deletion is before insertion', () => {
      const state = ChunkSelectState.fromText('aa\naa\n', 'bb\nbb\nbb\n', true);
      expect(renderLines(state)).toMatchObject([
        '[x] -  1    aa',
        '[x] -  2    aa',
        '[x] +     1 bb',
        '[x] +     2 bb',
        '[x] +     3 bb',
      ]);
    });
  });

  describe('setSelectedLines()', () => {
    it('toggles deleted lines', () => {
      let state = ChunkSelectState.fromText(a, b, false);
      state = state.setSelectedLines([
        [0, true],
        [1, false],
      ]);
      expect(renderLines(state).slice(0, 2)).toMatchObject(['[x] -  1    aa', '[ ] -  2    bb']);
      state = state.setSelectedLines([
        [0, false],
        [1, true],
      ]);
      expect(renderLines(state).slice(0, 2)).toMatchObject(['[ ] -  1    aa', '[x] -  2    bb']);
    });

    it('toggles added lines', () => {
      let state = ChunkSelectState.fromText(a, b, false);
      state = state.setSelectedLines([
        [4, true],
        [5, false],
      ]);
      expect(renderLines(state).slice(4, 6)).toMatchObject(['[x] +     3 ee', '[ ] +     4 ff']);
      state = state.setSelectedLines([
        [4, false],
        [5, true],
      ]);
      expect(renderLines(state).slice(4, 6)).toMatchObject(['[ ] +     3 ee', '[x] +     4 ff']);
    });

    it('does nothing to other lines', () => {
      const text = 'aa\ncc\n<insertion>\ndd\nee\n';
      let state = ChunkSelectState.fromText(a, b, text);
      state = state.setSelectedLines([
        [2, false],
        [3, false],
        [4, true],
      ]);
      expect(renderLines(state)).toMatchObject([
        '[ ] -  1    aa',
        '[x] -  2    bb',
        '       3  1 cc',
        '   !+       <insertion>',
        '       4  2 dd',
        '[x] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe(text);
    });
  });

  describe('setSelectedText()', () => {
    it('round-trips with getSelectedText()', () => {
      let state = ChunkSelectState.fromText(a, b, false);
      const lines = ['aa\n', 'bb\n', 'cc\n', 'ii\n', 'dd\n', 'ee\n', 'ff\n'];
      // eslint-disable-next-line no-bitwise
      const end = 1 << lines.length;
      for (let bits = 0; bits < end; ++bits) {
        // eslint-disable-next-line no-bitwise
        const text = lines.map((l, i) => ((bits & (1 << i)) === 0 ? '' : l)).join('');
        state = state.setSelectedText(text);
        expect(state.getSelectedText()).toBe(text);
      }
    });
  });

  describe('getInverseText()', () => {
    it('produces changes with inverse selection', () => {
      const state = ChunkSelectState.fromText(a, b, a).setSelectedLines([
        [0, true],
        [4, true],
      ]);
      expect(renderLines(state)).toMatchObject([
        '[x] -  1    aa',
        '[ ] -  2    bb',
        '       3  1 cc',
        '       4  2 dd',
        '[x] +     3 ee',
        '[ ] +     4 ff',
      ]);
      expect(state.getSelectedText()).toBe('bb\ncc\ndd\nee\n');
      expect(state.getInverseText()).toBe('aa\ncc\ndd\nff\n');
    });
  });

  describe('getLineRegions()', () => {
    it('produces a region when nothing is changed', () => {
      const state = ChunkSelectState.fromText(a, a, a);
      expect(state.getLineRegions()).toMatchObject([
        {
          lines: state.getLines(),
          same: true,
          collapsed: true,
        },
      ]);
    });

    it('produces a region when everything is changed', () => {
      const a = 'a\na\na\n';
      const b = 'b\nb\nb\nb\n';
      [a, b].forEach(m => {
        const state = ChunkSelectState.fromText(a, b, m);
        expect(state.getLineRegions()).toMatchObject([
          {
            lines: state.getLines(),
            same: false,
            collapsed: false,
          },
        ]);
      });
    });

    it('produces regions with complex changes', () => {
      const state = ChunkSelectState.fromText(
        '1\n2\n3\n4\n8\n9\n',
        '1\n2\n3\n5\n8\n9\n',
        '1\n2\n3\n5\n8\n9\n',
      );
      const lines = state.getLines();
      expect(state.getLineRegions()).toMatchObject([
        {
          lines: lines.slice(0, 1),
          collapsed: true,
          same: true,
        },
        {
          lines: lines.slice(1, 3),
          collapsed: false,
          same: true,
        },
        {
          lines: lines.slice(3, 5),
          collapsed: false,
          same: false,
        },
        {
          lines: lines.slice(5, 7),
          collapsed: false,
          same: true,
        },
      ]);
    });
  });
});

/** Visualize line selections in ASCII. */
function renderLines(state: ChunkSelectState): string[] {
  return state.getLines().map(l => {
    const checkbox = {true: '[x]', false: '[ ]', null: '   '}[`${l.selected}`];
    const aLine = l.aLine === null ? '' : l.aLine.toString();
    const bLine = l.bLine === null ? '' : l.bLine.toString();
    return [
      checkbox.padStart(3),
      l.sign.padStart(2),
      aLine.padStart(3),
      bLine.padStart(3),
      ' ',
      l.data.trimEnd(),
    ].join('');
  });
}
