/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export function stripStackInfoFromSaplingBodyHTML(bodyHTML: string): string {
  // This uses the same heuristic as ghstack, though note that it will NOT
  // work in the presence of sub-bullets.
  const delimiter = '</li>\n</ul>\n';
  const index = bodyHTML.indexOf(delimiter);
  // Retain any other lists that may be present as part of the commit message
  return index !== -1 ? bodyHTML.slice(index + delimiter.length) : bodyHTML;
}

/**
 * Sapling's built-in GitHub extension produces pull request bodies that
 * conform to the following rules:
 *
 * - The first line starts with "Stack created with [Sapling]". The intention
 *   is that the URL that follows "[Sapling]" can change over time
 *   (particularly if someone wants to run their own fork of ReviewStack),
 *   so we only match up to the square brackets.
 * - This is followed by zero or more lines that *do not* start with an
 *   asterisk. This likely contains instructional prose.
 * - There must be a block containing the list of pull requests in the stack.
 *   Each entry is line that starts with an asterisk, though leading
 *   whitespace is allowed so that individual commits for a pull request may
 *   be displayed as sub-bullets in the future.
 *   - For entries other than the current pull request, the asterisk must be
 *     followed by a single space, then #<PR>.
 *   - For the current pull request, there must be some other content before
 *     the #<PR> bit. The default signum is `__->__`, but we do not hardcode
 *     this so we can potentially support a "better looking" arrow in the
 *     future.
 *   - Content after the #<PR> is allowed, as we are considering things like
 *     (N commits) when N>1.
 * - The block defining the stack may not contain any empty lines. A sequence
 *   of two newlines (or `\r\n\r\n`) denotes the end of the stack.
 * - Everything after the two newlines is assumed to be the author's original
 *   commit message.
 */
export type SaplingPullRequestBody = {
  firstLine: string;
  introduction: string;
  /**
   * Each entry is the PR number and the number of commits in the PR
   * (from Sapling's perspective).
   */
  stack: Array<{number: number; numCommits: number}>;
  currentStackEntry: number;
  commitMessage: string;
};

const _STACK_SECTION_START = 'Stack created with [Sapling]';

export function parseSaplingStackBody(body: string): SaplingPullRequestBody | null {
  const lines = body.split(/\r?\n/);

  let firstLine: string;
  let index: number;
  let commitMessage = null;

  // A Sapling stack either starts with the _STACK_SECTION_START pattern, or has
  // a line starting with _STACK_SECTION_START after a horizontal rule.
  if (body.startsWith(_STACK_SECTION_START)) {
    firstLine = lines[0];
    index = 1;
  } else {
    const lastHRIndex = lines.lastIndexOf('---');
    if (lastHRIndex === -1 || !(lines[lastHRIndex + 1] ?? '').startsWith(_STACK_SECTION_START)) {
      return null;
    }

    firstLine = lines[lastHRIndex + 1];
    index = lastHRIndex + 2;
    commitMessage = lines.slice(0, lastHRIndex).join('\n');
    if (commitMessage !== '') {
      commitMessage += '\n';
    }
  }

  const introductionLines = [];
  const stack: Array<{number: number; numCommits: number}> = [];
  let inIntroduction = true;
  const numLines = lines.length;
  let currentStackEntry = null;
  while (index < numLines) {
    const line = lines[index++];

    if (inIntroduction) {
      if (/^\*/.test(line)) {
        inIntroduction = false;
      } else {
        introductionLines.push(line);
        continue;
      }
    }

    const match = line.match(/^\* (.*)#([1-9][0-9]*)(.*)$/);
    if (match != null) {
      const numCommitsMatch = match[3].match(/^\s*\(([1-9][0-9]*) commits\).*$/);
      const numCommits = numCommitsMatch != null ? parseInt(numCommitsMatch[1], 10) : 1;
      if (match[1] === '__->__ ') {
        if (currentStackEntry != null) {
          // Error: more than one currentStackEntry, so reject this body.
          return null;
        }
        currentStackEntry = stack.length;
      }
      stack.push({number: parseInt(match[2], 10), numCommits});
    } else if (/^\s+\*/.test(line)) {
      // This is a sub-bullet. We ignore these for now.
    } else if (line === '') {
      // This is the end of the block!
      break;
    } else {
      // This is suspicious: this is not a bulleted item, but there is
      // supposed be an extra blank line to delimit the commit message.
      // We'll break out, though.
      --index;
      break;
    }
  }

  if (currentStackEntry == null) {
    // Error: missing currentStackEntry, so reject this body.
    return null;
  }

  if (commitMessage == null) {
    commitMessage = lines.slice(index).join('\n');
  }

  return {
    firstLine,
    introduction: introductionLines.join('\n'),
    stack,
    currentStackEntry,
    commitMessage,
  };
}
