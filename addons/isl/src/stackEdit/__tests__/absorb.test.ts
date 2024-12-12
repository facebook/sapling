/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {analyseFileStack} from '../absorb';
import {FileStackState} from '../fileStackState';

// See also [test-fb-ext-absorb-filefixupstate.py](https://github.com/facebook/sapling/blob/eb3d35d/eden/scm/tests/test-fb-ext-absorb-filefixupstate.py#L75)
describe('analyseFileStack', () => {
  it('edits an empty file', () => {
    // Public: empty.
    const stack = createStack(['']);
    // No Selectedion - cannot edit the public (rev 0) content.
    expect(analyseFile(stack, 'a')).toMatchInlineSnapshot(`"0:0=>'a': Rev 0+ Selected null"`);
  });

  it('edits 2 lines by one insertion', () => {
    // Public: empty. Rev 1: "1\n1\n".
    const stack = createStack(['', '11']);
    // Delete the chunk.
    expect(analyseFile(stack, '')).toMatchInlineSnapshot(`"0:2=>'': Rev 1+ Selected 1"`);
    // Replace to 1 line.
    expect(analyseFile(stack, '2')).toMatchInlineSnapshot(`"0:2=>'2': Rev 1+ Selected 1"`);
    // Replace to 2 lines.
    expect(analyseFile(stack, '22')).toMatchInlineSnapshot(`"0:2=>'22': Rev 1+ Selected 1"`);
    // Replace to 3 lines.
    expect(analyseFile(stack, '222')).toMatchInlineSnapshot(`"0:2=>'222': Rev 1+ Selected 1"`);
  });

  it('edits 3 lines by 3 insertions', () => {
    // Public: empty. Rev 1: "1". Rev 2: "1", "2". Rev 3: "1", "2", "3".
    const stack = createStack(['', '1', '12', '123']);
    // No change.
    expect(analyseFile(stack, '123')).toMatchInlineSnapshot(`""`);
    // Replave the last line.
    expect(analyseFile(stack, '12c')).toMatchInlineSnapshot(`"2:3=>'c': Rev 3+ Selected 3"`);
    // Replave the 2nd line.
    expect(analyseFile(stack, '1b3')).toMatchInlineSnapshot(`"1:2=>'b': Rev 2+ Selected 2"`);
    // Replace the last 2 lines.
    expect(analyseFile(stack, '1bc')).toMatchInlineSnapshot(`
      "1:2=>'b': Rev 2+ Selected 2
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Replace the first line.
    expect(analyseFile(stack, 'a23')).toMatchInlineSnapshot(`"0:1=>'a': Rev 1+ Selected 1"`);
    // Replace the first and the last lines.
    expect(analyseFile(stack, 'a2c')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Replace the first 2 lines.
    expect(analyseFile(stack, 'ab3')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      1:2=>'b': Rev 2+ Selected 2"
    `);
    // Replace all 3 lines.
    expect(analyseFile(stack, 'abc')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      1:2=>'b': Rev 2+ Selected 2
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Non 1:1 line mapping.
    expect(analyseFile(stack, 'abcd')).toMatchInlineSnapshot(`"0:3=>'abcd': Rev 3+ Selected null"`);
    expect(analyseFile(stack, 'ab')).toMatchInlineSnapshot(`"0:3=>'ab': Rev 3+ Selected null"`);
    // Deletion.
    expect(analyseFile(stack, '')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      1:2=>'': Rev 2+ Selected 2
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '1')).toMatchInlineSnapshot(`
      "1:2=>'': Rev 2+ Selected 2
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '2')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '3')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      1:2=>'': Rev 2+ Selected 2"
    `);
    expect(analyseFile(stack, '13')).toMatchInlineSnapshot(`"1:2=>'': Rev 2+ Selected 2"`);
    // Replace the 2nd line with multiple lines.
    expect(analyseFile(stack, '1bb3')).toMatchInlineSnapshot(`"1:2=>'bb': Rev 2+ Selected 2"`);
    // "Confusing" replaces.
    expect(analyseFile(stack, '1bbb')).toMatchInlineSnapshot(`"1:3=>'bbb': Rev 3+ Selected null"`);
    expect(analyseFile(stack, 'bbb3')).toMatchInlineSnapshot(`"0:2=>'bbb': Rev 2+ Selected null"`);
    expect(analyseFile(stack, '1b')).toMatchInlineSnapshot(`"1:3=>'b': Rev 3+ Selected null"`);
    expect(analyseFile(stack, 'b3')).toMatchInlineSnapshot(`"0:2=>'b': Rev 2+ Selected null"`);
    // Insertion at the beginning and the end.
    expect(analyseFile(stack, '123c')).toMatchInlineSnapshot(`"3:3=>'c': Rev 3+ Selected 3"`);
    expect(analyseFile(stack, 'a123')).toMatchInlineSnapshot(`"0:0=>'a': Rev 1+ Selected 1"`);
    // "Confusing" insertions.
    expect(analyseFile(stack, '1a23')).toMatchInlineSnapshot(`"1:1=>'a': Rev 2+ Selected null"`);
    expect(analyseFile(stack, '12b3')).toMatchInlineSnapshot(`"2:2=>'b': Rev 3+ Selected null"`);
  });

  it('does not edit the public commit', () => {
    const stack = createStack(['1357', '0125678']);
    // Nothing changed.
    expect(analyseFile(stack, '0125678')).toMatchInlineSnapshot(`""`);
    // No Selectedion. "1" (from public) is changed to "a".
    expect(analyseFile(stack, '0a25678')).toMatchInlineSnapshot(`"1:2=>'a': Rev 0+ Selected null"`);
    // Whole block changed. NOTE: This is different from the Python behavior.
    expect(analyseFile(stack, 'abcdefg')).toMatchInlineSnapshot(
      `"0:7=>'abcdefg': Rev 1+ Selected 1"`,
    );
    expect(analyseFile(stack, 'abcdef')).toMatchInlineSnapshot(
      `"0:7=>'abcdef': Rev 1+ Selected 1"`,
    );
    expect(analyseFile(stack, '')).toMatchInlineSnapshot(`"0:7=>'': Rev 1+ Selected 1"`);
    // Insert 2 lines.
    expect(analyseFile(stack, '0123456789')).toMatchInlineSnapshot(`
      "3:3=>'34': Rev 1+ Selected 1
      7:7=>'9': Rev 1+ Selected 1"
    `);
  });

  function createStack(texts: string[]): FileStackState {
    return new FileStackState(texts.map(t => injectNewLines(t)));
  }

  function analyseFile(stack: FileStackState, newText: string): string {
    const chunks = analyseFileStack(stack, injectNewLines(newText));
    return chunks
      .map(
        c =>
          `${c.oldStart}:${c.oldEnd}=>'${c.newLines.map(l => l.replace('\n', '')).join('')}': Rev ${
            c.introductionRev
          }+ Selected ${c.selectedRev}`,
      )
      .join('\n');
  }

  /** Turn "abc" to "a\nb\nc\n". */
  function injectNewLines(text: string): string {
    return text
      .split('')
      .map(l => `${l}\n`)
      .join('');
  }
});
