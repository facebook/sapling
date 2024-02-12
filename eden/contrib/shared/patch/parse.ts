/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
  linedelimiters: string[];
}

export enum DiffType {
  Modified = 'Modified',
  Added = 'Added',
  Removed = 'Removed',
  Renamed = 'Renamed',
  Copied = 'Copied',
}

export interface ParsedDiff {
  type?: DiffType;
  oldFileName?: string;
  newFileName?: string;
  oldMode?: string;
  newMode?: string;
  hunks: Hunk[];
}

const DIFF = /^diff --git (.*) (.*)$/;
const RENAME_FROM = /^rename from (.*)$/;
const RENAME_TO = /^rename to (.*)$/;
const COPY_FROM = /^copy from (.*)$/;
const COPY_TO = /^copy to (.*)$/;
const NEW_FILE_MODE = /^new file mode (\d{6})$/;
const DELETED_FILE_MODE = /^deleted file mode (\d{6})$/;
const OLD_MODE = /^old mode (\d{6})$/;
const NEW_MODE = /^new mode (\d{6})$/;
const HUNK_HEADER = /@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;
const OLD_FILE_HEADER = /^--- (.*)$/;
const NEW_FILE_HEADER = /^\+\+\+ (.*)$/;

const DELIMITERS = /\r\n|[\n\v\f\r\x85]/g;

function assert(condition: unknown, msg?: string): asserts condition {
  if (condition === false) {
    throw new Error(msg);
  }
}

/**
 * Parse git diff format string.
 *
 * The diff library we were using does not support git diff format (rename,
 * copy, empty file, file mode change etc). This function is to extend the
 * original `parsePatch` function [1] and make it support git diff format [2].
 *
 * [1] https://github.com/DefinitelyTyped/DefinitelyTyped/blob/master/types/diff/index.d.ts#L388
 * [2] https://github.com/git/git-scm.com/blob/main/spec/data/diff-generate-patch.txt
 */
export function parsePatch(patch: string): ParsedDiff[] {
  const diffstr: string[] = patch.split(DELIMITERS);
  const delimiters: string[] = patch.match(DELIMITERS) || [];
  const list: ParsedDiff[] = [];
  let i = 0;

  function parseIndex() {
    const index: ParsedDiff = {hunks: []};
    list.push(index);

    parseHeader(index);

    // Parse one or more extended header lines
    while (i < diffstr.length) {
      const line = diffstr[i];
      if (/^old mode/.test(line)) {
        parseOldMode(index);
      } else if (/^new mode/.test(line)) {
        parseNewMode(index);
      } else if (/^deleted file mode/.test(line)) {
        parseDeletedFileMode(index);
      } else if (/^new file mode/.test(line)) {
        parseNewFileMode(index);
      } else if (/^copy /.test(line)) {
        parseCopy(index);
      } else if (/^rename /.test(line)) {
        parseRename(index);
      } else if (/^--- /.test(line)) {
        parseFileHeader(index);
        break;
      } else if (/^diff --git/.test(line)) {
        // a new index starts
        break;
      } else {
        // ignore other types (e.g. similarity etc)
        i++;
      }
    }

    parseHunks(index);
  }

  function parseHeader(index: ParsedDiff) {
    while (i < diffstr.length) {
      const line = diffstr[i];
      // Diff index
      const header = DIFF.exec(line);
      if (header) {
        index.oldFileName = header[1];
        index.newFileName = header[2];
        i++;
        break;
      }
      i++;
    }
  }

  function parseOldMode(index: ParsedDiff) {
    const arr = OLD_MODE.exec(diffstr[i]);
    assert(arr !== null, `invalid format '${diffstr[i]}'`);
    index.oldMode = arr[1];
    index.type = DiffType.Modified;
    i++;
  }

  function parseNewMode(index: ParsedDiff) {
    const arr = NEW_MODE.exec(diffstr[i]);
    assert(arr !== null, `invalid format '${diffstr[i]}'`);
    index.newMode = arr[1];
    index.type = DiffType.Modified;
    i++;
  }

  function parseDeletedFileMode(index: ParsedDiff) {
    const arr = DELETED_FILE_MODE.exec(diffstr[i]);
    assert(arr !== null, `invalid format '${diffstr[i]}'`);
    index.newMode = arr[1];
    index.type = DiffType.Removed;
    i++;
  }

  function parseNewFileMode(index: ParsedDiff) {
    const arr = NEW_FILE_MODE.exec(diffstr[i]);
    assert(arr !== null, `invalid format '${diffstr[i]}'`);
    index.newMode = arr[1];
    index.type = DiffType.Added;
    i++;
  }

  function parseCopy(index: ParsedDiff) {
    assert(COPY_FROM.test(diffstr[i]), `invalid format '${diffstr[i]}'`);
    assert(COPY_TO.test(diffstr[i + 1]), `invalid format '${diffstr[i + 1]}'`);
    index.type = DiffType.Copied;
    i += 2;
  }

  function parseRename(index: ParsedDiff) {
    assert(RENAME_FROM.test(diffstr[i]), `invalid format '${diffstr[i]}'`);
    assert(RENAME_TO.test(diffstr[i + 1]), `invalid format '${diffstr[i + 1]}'`);
    index.type = DiffType.Renamed;
    i += 2;
  }

  function parseFileHeader(index: ParsedDiff) {
    assert(OLD_FILE_HEADER.test(diffstr[i]), `invalid format '${diffstr[i]}'`);
    assert(NEW_FILE_HEADER.test(diffstr[i + 1]), `invalid format '${diffstr[i + 1]}'`);
    if (index.type === undefined) {
      index.type = DiffType.Modified;
    }
    i += 2;
  }

  function parseHunks(index: ParsedDiff) {
    while (i < diffstr.length) {
      const line = diffstr[i];
      if (DIFF.test(line)) {
        break;
      } else if (/^@@/.test(line)) {
        index.hunks.push(parseHunk());
      } else {
        // ignore unexpected content
        i++;
      }
    }
  }

  /*
   * Parses a hunk. This is copied from jsdiff library:
   * https://github.com/kpdecker/jsdiff/blob/master/src/patch/parse.js
   */
  function parseHunk(): Hunk {
    const hunkHeaderLine = diffstr[i++];
    const hunkHeader = hunkHeaderLine.split(HUNK_HEADER);

    const hunk: Hunk = {
      oldStart: +hunkHeader[1],
      oldLines: typeof hunkHeader[2] === 'undefined' ? 1 : +hunkHeader[2],
      newStart: +hunkHeader[3],
      newLines: typeof hunkHeader[4] === 'undefined' ? 1 : +hunkHeader[4],
      lines: [],
      linedelimiters: [],
    };

    // Unified Diff Format quirk: If the hunk size is 0,
    // the first number is one lower than one would expect.
    // https://www.artima.com/weblogs/viewpost.jsp?thread=164293
    if (hunk.oldLines === 0) {
      hunk.oldStart += 1;
    }
    if (hunk.newLines === 0) {
      hunk.newStart += 1;
    }

    let addCount = 0,
      removeCount = 0;
    for (; i < diffstr.length; i++) {
      // Lines starting with '---' could be mistaken for the "remove line" operation
      // But they could be the header for the next file. Therefore prune such cases out.
      if (
        diffstr[i].indexOf('--- ') === 0 &&
        i + 2 < diffstr.length &&
        diffstr[i + 1].indexOf('+++ ') === 0 &&
        diffstr[i + 2].indexOf('@@') === 0
      ) {
        break;
      }
      const operation = diffstr[i].length == 0 && i != diffstr.length - 1 ? ' ' : diffstr[i][0];

      if (operation === '+' || operation === '-' || operation === ' ' || operation === '\\') {
        hunk.lines.push(diffstr[i]);
        hunk.linedelimiters.push(delimiters[i] || '\n');

        if (operation === '+') {
          addCount++;
        } else if (operation === '-') {
          removeCount++;
        } else if (operation === ' ') {
          addCount++;
          removeCount++;
        }
      } else {
        break;
      }
    }

    // Handle the empty block count case
    if (!addCount && hunk.newLines === 1) {
      hunk.newLines = 0;
    }
    if (!removeCount && hunk.oldLines === 1) {
      hunk.oldLines = 0;
    }

    return hunk;
  }

  while (i < diffstr.length) {
    parseIndex();
  }

  return list;
}
