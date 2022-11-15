/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hunk} from 'diff';

import organizeLinesIntoGroups from '../../SplitDiffView/organizeLinesIntoGroups';
import {structuredPatch} from 'diff';

test('file with only one line that is changed (no context)', () => {
  const hunks = diffIntoHunks(['lowerCamelCase'], ['UpperCamelCase']);
  expect(hunks.length).toBe(1);
  const groups = organizeLinesIntoGroups(hunks[0].lines);
  expect(groups).toEqual([{common: [], removed: ['lowerCamelCase'], added: ['UpperCamelCase']}]);
});

test('file with only first line changed', () => {
  const hunks = diffIntoHunks(['lowerCamelCase', 'a', 'b', 'c'], ['UpperCamelCase', 'a', 'b', 'c']);
  expect(hunks.length).toBe(1);
  const groups = organizeLinesIntoGroups(hunks[0].lines);
  expect(groups).toEqual([
    {common: [], removed: ['lowerCamelCase'], added: ['UpperCamelCase']},
    {common: ['a', 'b', 'c'], removed: [], added: []},
  ]);
});

test('file with only last line changed', () => {
  const hunks = diffIntoHunks(['a', 'b', 'c', 'lowerCamelCase'], ['a', 'b', 'c', 'UpperCamelCase']);
  expect(hunks.length).toBe(1);
  const groups = organizeLinesIntoGroups(hunks[0].lines);
  expect(groups).toEqual([
    {common: ['a', 'b', 'c'], removed: ['lowerCamelCase'], added: ['UpperCamelCase']},
  ]);
});

test('a mix of changed lines', () => {
  const hunks = diffIntoHunks(
    ['...', 'The', 'quick', 'fox', 'jumped', 'over', 'dog.', 'THE END'],
    ['The', 'quick', 'BROWN', 'fox', 'jumps', 'over', 'the lazy dog.', 'THE END'],
  );
  expect(hunks.length).toBe(1);
  const groups = organizeLinesIntoGroups(hunks[0].lines);
  expect(groups).toEqual([
    {
      common: [],
      removed: ['...'],
      added: [],
    },
    {
      common: ['The', 'quick'],
      removed: [],
      added: ['BROWN'],
    },
    {
      common: ['fox'],
      removed: ['jumped'],
      added: ['jumps'],
    },
    {
      common: ['over'],
      removed: ['dog.'],
      added: ['the lazy dog.'],
    },
    {
      common: ['THE END'],
      removed: [],
      added: [],
    },
  ]);
});

function diffIntoHunks(oldLines: string[], newLines: string[], context = 3): Hunk[] {
  const oldText = oldLines.join('\n') + '\n';
  const newText = newLines.join('\n') + '\n';
  const parsedDiff = structuredPatch('old.txt', 'new.txt', oldText, newText, undefined, undefined, {
    context,
  });
  return parsedDiff.hunks;
}
