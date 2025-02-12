/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Map as ImMap} from 'immutable';
import type {AbsorbEdit, AbsorbEditId} from '../absorb';
import type {FileRev} from '../fileStackState';

import {splitLines} from 'shared/diff';
import {
  analyseFileStack,
  applyFileStackEditsWithAbsorbId,
  calculateAbsorbEditsForFileStack,
  embedAbsorbId,
  extractRevAbsorbId,
  revWithAbsorb,
} from '../absorb';
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
    const stack = createStack(['', '1↵1↵']);
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
    const stack = createStack(['', '1↵', '1↵2↵', '1↵2↵3↵']);
    // No change.
    expect(analyseFile(stack, '1↵2↵3↵')).toMatchInlineSnapshot(`""`);
    // Replave the last line.
    expect(analyseFile(stack, '1↵2↵c↵')).toMatchInlineSnapshot(`"2:3=>'c': Rev 3+ Selected 3"`);
    // Replave the 2nd line.
    expect(analyseFile(stack, '1↵b↵3↵')).toMatchInlineSnapshot(`"1:2=>'b': Rev 2+ Selected 2"`);
    // Replace the last 2 lines.
    expect(analyseFile(stack, '1↵b↵c↵')).toMatchInlineSnapshot(`
      "1:2=>'b': Rev 2+ Selected 2
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Replace the first line.
    expect(analyseFile(stack, 'a↵2↵3↵')).toMatchInlineSnapshot(`"0:1=>'a': Rev 1+ Selected 1"`);
    // Replace the first and the last lines.
    expect(analyseFile(stack, 'a↵2↵c↵')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Replace the first 2 lines.
    expect(analyseFile(stack, 'a↵b↵3↵')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      1:2=>'b': Rev 2+ Selected 2"
    `);
    // Replace all 3 lines.
    expect(analyseFile(stack, 'a↵b↵c↵')).toMatchInlineSnapshot(`
      "0:1=>'a': Rev 1+ Selected 1
      1:2=>'b': Rev 2+ Selected 2
      2:3=>'c': Rev 3+ Selected 3"
    `);
    // Non 1:1 line mapping.
    expect(analyseFile(stack, 'a↵b↵c↵d↵')).toMatchInlineSnapshot(
      `"0:3=>'abcd': Rev 3+ Selected null"`,
    );
    expect(analyseFile(stack, 'a↵b↵')).toMatchInlineSnapshot(`"0:3=>'ab': Rev 3+ Selected null"`);
    // Deletion.
    expect(analyseFile(stack, '')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      1:2=>'': Rev 2+ Selected 2
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '1↵')).toMatchInlineSnapshot(`
      "1:2=>'': Rev 2+ Selected 2
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '2↵')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      2:3=>'': Rev 3+ Selected 3"
    `);
    expect(analyseFile(stack, '3↵')).toMatchInlineSnapshot(`
      "0:1=>'': Rev 1+ Selected 1
      1:2=>'': Rev 2+ Selected 2"
    `);
    expect(analyseFile(stack, '1↵3↵')).toMatchInlineSnapshot(`"1:2=>'': Rev 2+ Selected 2"`);
    // Replace the 2nd line with multiple lines.
    expect(analyseFile(stack, '1↵b↵b↵3↵')).toMatchInlineSnapshot(`"1:2=>'bb': Rev 2+ Selected 2"`);
    // "Confusing" replaces.
    expect(analyseFile(stack, '1↵b↵b↵b↵')).toMatchInlineSnapshot(
      `"1:3=>'bbb': Rev 3+ Selected null"`,
    );
    expect(analyseFile(stack, 'b↵b↵b↵3↵')).toMatchInlineSnapshot(
      `"0:2=>'bbb': Rev 2+ Selected null"`,
    );
    expect(analyseFile(stack, '1↵b↵')).toMatchInlineSnapshot(`"1:3=>'b': Rev 3+ Selected null"`);
    expect(analyseFile(stack, 'b↵3↵')).toMatchInlineSnapshot(`"0:2=>'b': Rev 2+ Selected null"`);
    // Insertion at the beginning and the end.
    expect(analyseFile(stack, '1↵2↵3↵c↵')).toMatchInlineSnapshot(`"3:3=>'c': Rev 3+ Selected 3"`);
    expect(analyseFile(stack, 'a↵1↵2↵3↵')).toMatchInlineSnapshot(`"0:0=>'a': Rev 1+ Selected 1"`);
    // "Confusing" insertions.
    expect(analyseFile(stack, '1↵a↵2↵3↵')).toMatchInlineSnapshot(
      `"1:1=>'a': Rev 2+ Selected null"`,
    );
    expect(analyseFile(stack, '1↵2↵b↵3↵')).toMatchInlineSnapshot(
      `"2:2=>'b': Rev 3+ Selected null"`,
    );
  });

  it('does not edit the public commit', () => {
    const stack = createStack(['1↵3↵5↵7↵', '0↵1↵2↵5↵6↵7↵8↵']);
    // Nothing changed.
    expect(analyseFile(stack, '0↵1↵2↵5↵6↵7↵8↵')).toMatchInlineSnapshot(`""`);
    // No Selectedion. "1" (from public) is changed to "a".
    expect(analyseFile(stack, '0↵a↵2↵5↵6↵7↵8↵')).toMatchInlineSnapshot(
      `"1:2=>'a': Rev 0+ Selected null"`,
    );
    // Whole block changed. NOTE: This is different from the Python behavior.
    expect(analyseFile(stack, 'a↵b↵c↵d↵e↵f↵g↵')).toMatchInlineSnapshot(
      `"0:7=>'abcdefg': Rev 1+ Selected 1"`,
    );
    expect(analyseFile(stack, 'a↵b↵c↵d↵e↵f↵')).toMatchInlineSnapshot(
      `"0:7=>'abcdef': Rev 1+ Selected 1"`,
    );
    expect(analyseFile(stack, '')).toMatchInlineSnapshot(`"0:7=>'': Rev 1+ Selected 1"`);
    // Insert 2 lines.
    expect(analyseFile(stack, '0↵1↵2↵3↵4↵5↵6↵7↵8↵9↵')).toMatchInlineSnapshot(`
      "3:3=>'34': Rev 1+ Selected 1
      7:7=>'9': Rev 1+ Selected 1"
    `);
  });

  describe('applyFileStackEdits', () => {
    it('edits 3 lines by 3 insertions', () => {
      // Replace ['1','2','3'] to ['a','b','c'], 1->a, 2->b, 3->c.
      const fullStack = createStack(['', '1↵', '1↵2↵', '1↵2↵3↵', 'a↵b↵c↵']);
      const edits = calculateAbsorbEditsForFileStack(fullStack)[1];
      const stack = fullStack.truncate((fullStack.revLength - 1) as FileRev);
      expect(applyEdits(stack, edits.values())).toMatchInlineSnapshot(`" a↵ a↵b↵ a↵b↵c↵"`);
      // Tweak the `selectedRev` so the 1->a, 2->b changes happen at the last rev.
      const edits2 = edits.map(c => c.set('selectedRev', 3 as FileRev));
      expect(applyEdits(stack, edits2.values())).toMatchInlineSnapshot(`" 1↵ 1↵2↵ a↵b↵c↵"`);
      // Drop the "2->b" change by setting selectedRev to `null`.
      const edits3 = edits.map(c => (c.oldStart === 1 ? c.set('selectedRev', null) : c));
      expect(applyEdits(stack, edits3.values())).toMatchInlineSnapshot(`" a↵ a↵2↵ a↵2↵c↵ a↵b↵c↵"`);
    });

    it('edits do not need to be 1:1 line mapping', () => {
      // Replace ['111','2','333'] to ['aaaa','2','cc']. 111->aaaa. 333->cc.
      const fullStack = createStack(['', '2↵', '1↵1↵1↵2↵3↵3↵3↵', 'a↵a↵a↵a↵2↵c↵c↵']);
      const edits = calculateAbsorbEditsForFileStack(fullStack)[1];
      const stack = fullStack.truncate((fullStack.revLength - 1) as FileRev);
      expect(applyEdits(stack, edits.values())).toMatchInlineSnapshot(`" 2↵ a↵a↵a↵a↵2↵c↵c↵"`);
      // Drop the "1->aaa" change by setting selectedRev to `null`.
      const edits2 = edits.map(c => (c.oldStart === 0 ? c.set('selectedRev', null) : c));
      expect(applyEdits(stack, edits2.values())).toMatchInlineSnapshot(
        `" 2↵ 1↵1↵1↵2↵c↵c↵ a↵a↵a↵a↵2↵c↵c↵"`,
      );
    });
  });

  describe('absorbId', () => {
    it('can be embedded into rev, and extracted out', () => {
      const plainRev = 567;
      const absorbEditId = 890;
      const rev = embedAbsorbId(plainRev as FileRev, absorbEditId);
      expect(extractRevAbsorbId(rev)).toEqual([plainRev, absorbEditId]);
    });
  });

  describe('calculateAbsorbEditsForFileStack', () => {
    it('analyses a stack', () => {
      const stack = createStack([
        'p↵u↵b↵',
        'p↵u↵b↵1↵2↵3↵4↵',
        'p↵u↵b↵2↵3↵4↵5↵6↵',
        'p↵U↵b↵x↵3↵4↵6↵y↵',
      ]);
      const [analysedStack, absorbMap] = calculateAbsorbEditsForFileStack(stack);
      expect(describeAbsorbIdChunkMap(absorbMap)).toMatchInlineSnapshot(`
        [
          "0: -u↵ +U↵ Introduced=0",
          "1: -2↵ +x↵ Selected=1 Introduced=1",
          "2: -5↵ Selected=2 Introduced=2",
          "3: +y↵ Selected=2 Introduced=2",
        ]
      `);
      const show = (rev: number) => compactText(analysedStack.getRev(rev as FileRev));
      // Rev 1 original.
      expect(show(1)).toMatchInlineSnapshot(`"p↵u↵b↵1↵2↵3↵4↵"`);
      // Rev 1.99 is Rev 1 with the absorb "-2 -x" chunk applied.
      expect(show(1.99)).toMatchInlineSnapshot(`"p↵u↵b↵1↵x↵3↵4↵"`);
      // Rev 2 original.
      expect(show(2)).toMatchInlineSnapshot(`"p↵u↵b↵x↵3↵4↵5↵6↵"`);
      // Rev 2.99 is Rev 2 with the absorb "-5 +y" applied.
      const rev299 = revWithAbsorb(2 as FileRev);
      expect(show(rev299)).toMatchInlineSnapshot(`"p↵u↵b↵x↵3↵4↵6↵y↵"`);
      // Rev 3 "wdir()" is dropped - no changes from 2.99.
      expect(show(3)).toMatchInlineSnapshot(`"p↵u↵b↵x↵3↵4↵6↵y↵"`);
      // Rev 3.99 includes changes left in "wdir()": "pub" -> "pUb".
      // This edit changes the "public" portion so it wasn't absorbed by default.
      expect(show(3.99)).toMatchInlineSnapshot(`"p↵U↵b↵x↵3↵4↵6↵y↵"`);
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

    it('1:1 line mapping edit can include immutable lines', () => {
      const stack = createStack(['p↵', 'p↵1↵', 'p↵1↵2↵', 'P↵X↵Y↵']);
      const [, absorbMap] = calculateAbsorbEditsForFileStack(stack);
      // Absorbed edits: "1 => X"; "2 => Y" (selected is set)
      // "p => P" is left in the working copy, since "p" is considered immutable.
      expect(describeAbsorbIdChunkMap(absorbMap)).toMatchInlineSnapshot(`
        [
          "0: -p↵ +P↵ Introduced=0",
          "1: -1↵ +X↵ Selected=1 Introduced=1",
          "2: -2↵ +Y↵ Selected=2 Introduced=2",
        ]
      `);
    });
  });

  function createStack(texts: string[]): FileStackState {
    return new FileStackState(texts.map(t => injectNewLines(t)));
  }

  function analyseFile(stack: FileStackState, newText: string): string {
    const text = injectNewLines(newText);
    const oldLines = splitLines(stack.getRev((stack.revLength - 1) as FileRev));
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

  function applyEdits(stack: FileStackState, edits: Iterable<AbsorbEdit>): string {
    const editedStack = applyFileStackEditsWithAbsorbId(stack, edits);
    return compactTexts(editedStack.revs().map(rev => editedStack.getRev(revWithAbsorb(rev))));
  }

  /** Replace "↵" with "\n" */
  function injectNewLines(text: string): string {
    return text.replaceAll('↵', '\n');
  }

  function compactText(text: string): string {
    return text.replaceAll('\n', '↵');
  }
});

/** Turn ["a\n", "a\nb\n"] to "a↵ ab↵". */
function compactTexts(texts: Iterable<string>): string {
  return [...texts].map(t => t.replaceAll('\n', '↵')).join(' ');
}

export function describeAbsorbIdChunkMap(map: ImMap<AbsorbEditId, AbsorbEdit>): string[] {
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
