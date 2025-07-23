/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {CommitInfo} from 'isl/src/types';

import {relativeDate} from 'isl/src/relativeDate';

export function getDiffBlameHoverMarkup(repo: Repository, commit: CommitInfo): string {
  const {date, author, title, hash} = commit;

  let diffId = commit.diffId;
  if (diffId == null) {
    // Hack: Public commits in GitHub-backed repos often don't have a PR number associated.
    // Do a best-effort match in the title/description.
    // TODO: we should see if we can fix this in sl itself.
    const PRnumberRegex = /#(\d{2,}\b)/; // Sure, this misses the first 9 PRs, but also avoids "#1" reason for false positives.
    diffId = commit.title.match(PRnumberRegex)?.[1] ?? commit.description.match(PRnumberRegex)?.[1];
  }

  const diffLinkMarkup =
    diffId != null
      ? `${repo.codeReviewProvider?.getDiffUrlMarkdown(diffId)}`
      : (repo.codeReviewProvider?.getCommitHashUrlMarkdown(commit.hash) ?? hash.slice(0, 12));

  // Though the ISL UI knows the schema for commit messages and can parse things out,
  // here in the vscode extension we don't really know the schema. Let's just dump the whole commit message.
  return (
    `**${author}** - ${diffLinkMarkup} (${relativeDate(date, {})})\n\n` +
    `**${title}**\n\n\n${commit.description}`
  );
}
