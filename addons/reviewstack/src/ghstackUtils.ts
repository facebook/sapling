/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {notEmpty} from 'shared/utils';

export function pullRequestNumbersFromBody(body: string): number[] | null {
  if (!body.startsWith('Stack from [ghstack]')) {
    return null;
  }

  // Info inserted by ghstack is separated from the rest of the body by a blank line
  const stackInfo = body.split(/\r?\n\r?\n/, 1)[0];
  // The first line is the stack header, the rest is a bulleted list of pull requests
  const pullRequestLines = stackInfo.split(/\r?\n/).slice(1);

  return pullRequestLines
    .map(line => {
      // Get the pull request number, which is prefixed by #
      const match = line.match(/#(\d+)$/);
      if (match == null) {
        return null;
      }
      return parseInt(match[1], 10);
    })
    .filter(notEmpty);
}

export function stripStackInfoFromBodyHTML(bodyHTML: string): string {
  // The header and list info inserted by ghstack is followed by a newline
  const delimiter = '</li>\n</ul>\n';
  const index = bodyHTML.indexOf(delimiter);
  // Retain any other lists that may be present as part of the commit message
  return index !== -1 ? bodyHTML.slice(index + delimiter.length) : bodyHTML;
}
