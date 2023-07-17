/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {PartialSelection} from '../partialSelection';

describe('PartialSelection', () => {
  it('constructs with empty()', () => {
    let select = PartialSelection.empty({selectByDefault: true});
    expect(select.isEverythingSelected(() => ['path1'])).toBe(true);
    expect(select.isEverythingSelected(() => [])).toBe(true);
    expect(select.isNothingSelected(() => ['path1'])).toBe(false);
    expect(select.isNothingSelected(() => [])).toBe(true);

    select = PartialSelection.empty({selectByDefault: false});
    expect(select.isEverythingSelected(() => ['path1'])).toBe(false);
    expect(select.isEverythingSelected(() => [])).toBe(true);
    expect(select.isNothingSelected(() => ['path1'])).toBe(true);
    expect(select.isNothingSelected(() => [])).toBe(true);
  });

  it('selects files using select() and deselect()', () => {
    [true, false].forEach(selectByDefault => {
      let select = PartialSelection.empty({selectByDefault}).select('path1').deselect('path2');
      expect(select.isEverythingSelected(() => ['path1', 'path2'])).toBe(false);
      expect(select.isNothingSelected(() => ['path1', 'path2'])).toBe(false);
      expect(select.getSimplifiedSelection('path1')).toBe(true);
      expect(select.getSimplifiedSelection('path2')).toBe(false);

      select = select.select('path2');
      expect(select.getSimplifiedSelection('path2')).toBe(true);
      expect(select.isEverythingSelected(() => ['path1', 'path2'])).toBe(true);

      select = select.deselect('path2').deselect('path1');
      expect(select.getSimplifiedSelection('path1')).toBe(false);
      expect(select.isNothingSelected(() => ['path1', 'path2'])).toBe(true);
    });
  });

  it('clear() removes selection', () => {
    [true, false].forEach(selectByDefault => {
      const select = PartialSelection.empty({selectByDefault})
        .select('path1')
        .deselect('path2')
        .clear();
      expect(select.getSimplifiedSelection('path1')).toBe(selectByDefault);
      expect(select.getSimplifiedSelection('path2')).toBe(selectByDefault);
    });
  });

  it('partially selects files using startChunkSelect() and editChunkSelect()', () => {
    [true, false].forEach(selectByDefault => {
      let select = PartialSelection.empty({selectByDefault});
      select = select.startChunkSelect('path1', '11\n22\n', '22\n33\n', false);
      expect(select.getSimplifiedSelection('path1')).toBe(false);
      expect(select.isNothingSelected(() => ['path1'])).toBe(true);
      expect(select.isEverythingSelected(() => ['path1'])).toBe(false);

      select = select.startChunkSelect('path1', '11\n22\n', '22\n33\n', true);
      expect(select.getSimplifiedSelection('path1')).toBe(true);
      expect(select.isNothingSelected(() => ['path1'])).toBe(false);
      expect(select.isEverythingSelected(() => ['path1'])).toBe(true);

      select = select.editChunkSelect('path1', chunkState =>
        chunkState.setSelectedLines([
          [0, false], // deselect '- 11'.
          [2, true], // select '+ 33'.
        ]),
      );
      expect(select.getSimplifiedSelection('path1')).toBe('11\n22\n33\n');
      expect(select.isNothingSelected(() => ['path1'])).toBe(false);
      expect(select.isEverythingSelected(() => ['path1'])).toBe(false);
    });
  });

  it('calculates ImportStackFiles', () => {
    [true, false].forEach(selectByDefault => {
      [true, false].forEach(inverse => {
        const select = PartialSelection.empty({selectByDefault})
          .select('path1')
          .deselect('path2')
          .startChunkSelect('path3', 'a\n', 'b\n', 'a\n')
          .startChunkSelect('path4', 'a\n', 'b\n', 'b\n')
          .startChunkSelect('path5', 'a\n', 'b\n', 'c\n');
        const allPaths = ['path1', 'path2', 'path3', 'path4', 'path5', 'path6'];
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const expected: any = {
          path1: '.',
          path4: {data: inverse ? 'a\n' : 'b\n', copyFrom: '.', flags: '.'},
          path5: {data: inverse ? 'b\na\n' : 'c\n', copyFrom: '.', flags: '.'},
        };
        if (selectByDefault) {
          expected.path6 = '.';
        }
        expect(select.calculateImportStackFiles(allPaths, inverse)).toMatchObject(expected);
      });
    });
  });
});
