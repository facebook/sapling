/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from 'isl/src/types';

import {relativeDate} from 'isl/src/relativeDate';

export function getDiffBlameHoverMarkup(commit: CommitInfo): string {
  const {date, author, title, diffId, hash} = commit;
  const diffLink = diffId != null ? `[${diffId}](${diffId})` : hash;

  // Though the ISL UI knows the schema for commit messages and can parse things out,
  // here in the vscode extension we don't really know the schema. Let's just dump the whole commit message.
  return (
    `**${author}** - ${diffLink} (${relativeDate(date, {})})\n\n` +
    `**${title}**\n\n\n${commit.description}`
  );
}
