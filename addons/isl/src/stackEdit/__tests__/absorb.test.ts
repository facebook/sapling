/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AbsorbDiffChunk, AbsorbEditId} from '../absorb';
import type {Rev} from '../fileStackState';
import type {List, Map as ImMap} from 'immutable';

import {
  analyseFileStack,
  applyFileStackEdits,
  calculateAbsorbEditsForFileStack,
  embedAbsorbId,
  extractRevAbsorbId,
  revWithAbsorb,
} from '../absorb';
import {FileStackState} from '../fileStackState';
import {splitLines} from 'shared/diff';

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

  describe('applyFileStackEdits', () => {
    it('edits 3 lines by 3 insertions', () => {
      // Replace ['1','2','3'] to ['a','b','c'], 1->a, 2->b, 3->c.
      const stack = createStack(['', '1', '12', '123']);
      const chunks = analyseFileStack(stack, injectNewLines('abc'));
      expect(applyChunks(stack, chunks)).toMatchInlineSnapshot(`" a ab abc"`);
      // Tweak the `selectedRev` so the 1->a, 2->b changes happen at the last rev.
      const chunks2 = chunks.map(c => c.set('selectedRev', 3));
      expect(applyChunks(stack, chunks2)).toMatchInlineSnapshot(`" 1 12 abc"`);
      // Drop the "2->b" change by setting selectedRev to `null`.
      const chunks3 = chunks.map(c => (c.oldStart === 1 ? c.set('selectedRev', null) : c));
      expect(applyChunks(stack, chunks3)).toMatchInlineSnapshot(`" a a2 a2c"`);
    });

    it('edits do not need to be 1:1 line mapping', () => {
      // Replace ['111','2','333'] to ['aaaa','2','cc']. 111->aaaa. 333->cc.
      const stack = createStack(['', '2', '1112333']);
      const chunks = analyseFileStack(stack, injectNewLines('aaaa2cc'));
      expect(applyChunks(stack, chunks)).toMatchInlineSnapshot(`" 2 aaaa2cc"`);
      // Drop the "1->aaa" change by setting selectedRev to `null`.
      const chunks3 = chunks.map(c => (c.oldStart === 0 ? {...c, selectedRev: null} : c));
      expect(applyChunks(stack, chunks3)).toMatchInlineSnapshot(`" 2 1112cc"`);
    });
  });

  describe('absorbId', () => {
    it('can be embedded into rev, and extracted out', () => {
      const plainRev = 567;
      const absorbEditId = 890;
      const rev = embedAbsorbId(plainRev, absorbEditId);
      expect(extractRevAbsorbId(rev)).toEqual([plainRev, absorbEditId]);
    });
  });

  describe('calculateAbsorbEditsForFileStack', () => {
    it('analyses a stack', () => {
      const stack = createStack(['pub', 'pub1234', 'pub23456', 'pUbx346y']);
      const [analysedStack, absorbMap] = calculateAbsorbEditsForFileStack(stack);
      expect(describeAbsorbIdChunkMap(absorbMap)).toMatchInlineSnapshot(`
        [
          "0: -u +U Introduced=0",
          "1: -2 +x Selected=1 Introduced=1",
          "2: -5 Selected=2 Introduced=2",
          "3: +y Selected=2 Introduced=2",
        ]
      `);
      const show = (rev: Rev) => compactText(analysedStack.getRev(rev));
      // Rev 1 original.
      expect(show(1)).toMatchInlineSnapshot(`"pub1234"`);
      // Rev 1.99 is Rev 1 with the absorb "-2 -x" chunk applied.
      expect(show(1.99)).toMatchInlineSnapshot(`"pub1x34"`);
      // Rev 2 original.
      expect(show(2)).toMatchInlineSnapshot(`"pubx3456"`);
      // Rev 2.99 is Rev 2 with the absorb "-5 +y" applied.
      const rev299 = revWithAbsorb(2);
      expect(show(rev299)).toMatchInlineSnapshot(`"pubx346y"`);
      // Rev 3 "wdir()" is dropped - no changes from 2.99.
      expect(show(3)).toMatchInlineSnapshot(`"pubx346y"`);
      // Rev 3.99 includes changes left in "wdir()": "pub" -> "pUb".
      // This edit changes the "public" portion so it wasn't absorbed by default.
      expect(show(3.99)).toMatchInlineSnapshot(`"pUbx346y"`);
      expect(analysedStack.convertToLineLog().code.describeHumanReadableInstructions())
        .toMatchInlineSnapshot(`
        [
          "0: J 1",
          "1: JL 0 5",
          "2: LINE 0 "p"",
          "3: J 30",
          "4: LINE 0 "b"",
          "5: J 6",
          "6: JL 1 11",
          "7: J 16",
          "8: J 25",
          "9: LINE 1 "3"",
          "10: LINE 1 "4"",
          "11: J 12",
          "12: JL 2 15",
          "13: J 22",
          "14: LINE 2 "6"",
          "15: J 19",
          "16: JGE 2 8",
          "17: LINE 1 "1"",
          "18: J 8",
          "19: J 21",
          "20: J 21",
          "21: J 35",
          "22: J 23",
          "23: J 38",
          "24: J 14",
          "25: J 27",
          "26: J 27",
          "27: J 28",
          "28: J 41",
          "29: J 9",
          "30: J 32",
          "31: J 32",
          "32: J 33",
          "33: J 46",
          "34: J 4",
          "35: JL 2.0000038146972656 37",
          "36: LINE 2.0000038146972656 "y"",
          "37: END",
          "38: JGE 2.000002861022949 24",
          "39: LINE 2 "5"",
          "40: J 24",
          "41: JL 1.0000019073486328 43",
          "42: LINE 1.0000019073486328 "x"",
          "43: JGE 1.0000019073486328 29",
          "44: LINE 1 "2"",
          "45: J 29",
          "46: JL 3.0000009536743164 48",
          "47: LINE 3.0000009536743164 "U"",
          "48: JGE 3.0000009536743164 34",
          "49: LINE 0 "u"",
          "50: J 34",
        ]
      `);
    });
  });

  function createStack(texts: string[]): FileStackState {
    return new FileStackState(texts.map(t => injectNewLines(t)));
  }

  function analyseFile(stack: FileStackState, newText: string): string {
    const text = injectNewLines(newText);
    const oldLines = splitLines(stack.getRev(stack.revLength - 1));
    const newLines = splitLines(text);
    const chunks = analyseFileStack(stack, text);
    return chunks
      .map(c => {
        // Check the old and new line numbers and content match.
        expect(oldLines.slice(c.oldStart, c.oldEnd)).toEqual(c.oldLines.toArray());
        expect(newLines.slice(c.newStart, c.newEnd)).toEqual(c.newLines.toArray());
        return `${c.oldStart}:${c.oldEnd}=>'${c.newLines
          .map(l => l.replace('\n', ''))
          .join('')}': Rev ${c.introductionRev}+ Selected ${c.selectedRev}`;
      })
      .join('\n');
  }

  function applyChunks(stack: FileStackState, chunks: Iterable<AbsorbDiffChunk>): string {
    return compactTexts(applyFileStackEdits(stack, chunks).convertToPlainText());
  }

  /** Turn "abc" to "a\nb\nc\n". */
  function injectNewLines(text: string): string {
    return text
      .split('')
      .map(l => `${l}\n`)
      .join('');
  }

  function compactText(text: string): string {
    return text.replaceAll('\n', '');
  }
});

/** Turn ["a\n", "a\nb\n"] to "a ab". */
function compactTexts(texts: List<string>): string {
  return texts.map(t => t.replace(/\n/g, '')).join(' ');
}

export function describeAbsorbIdChunkMap(map: ImMap<AbsorbEditId, AbsorbDiffChunk>): string[] {
  const result: string[] = [];
  map.forEach((chunk, id) => {
    const words: string[] = [`${id}:`];
    if (!chunk.oldLines.isEmpty()) {
      words.push(`-${compactTexts(chunk.oldLines)}`);
    }
    if (!chunk.newLines.isEmpty()) {
      words.push(`+${compactTexts(chunk.newLines)}`);
    }
    if (chunk.selectedRev != null) {
      words.push(`Selected=${chunk.selectedRev}`);
    }
    words.push(`Introduced=${chunk.introductionRev}`);
    result.push(words.join(' '));
  });
  return result;
}
