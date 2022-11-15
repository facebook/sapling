/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {HomePagePullRequestFragment, UserHomePageQueryData} from './generated/graphql';

import CenteredSpinner from './CenteredSpinner';
import Link from './Link';
import PullRequestStateLabel from './PullRequestStateLabel';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubUserHomePageData} from './recoil';
import {Box, Heading, Text} from '@primer/react';
import {Suspense} from 'react';
import {useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

export default function UserHomePage(): React.ReactElement {
  return (
    <Suspense fallback={<CenteredSpinner />}>
      <UserHomePageRoot />
    </Suspense>
  );
}

function UserHomePageRoot(): React.ReactElement {
  const data = useRecoilValue(gitHubUserHomePageData);
  return (
    <Box>
      <ReviewRequestsForUser reviewRequests={data?.search.nodes ?? []} />
      <PullRequestsForUser pullRequests={data?.viewer.pullRequests.nodes ?? []} />
      <RepositoriesForUser repos={data?.viewer.repositories.nodes ?? []} />
    </Box>
  );
}

function PullRequests({
  pullRequests,
}: {
  pullRequests: Array<HomePagePullRequestFragment | null>;
}): React.ReactElement {
  const pullRequestList = pullRequests.map((pullRequest, index) => {
    if (pullRequest == null) {
      return null;
    }

    const {number, titleHTML, repository, state, reviewDecision} = pullRequest;
    const {nameWithOwner} = repository;
    return (
      <Box key={index}>
        <Box sx={{display: 'inline-block', width: 100, pb: 2}}>
          <PullRequestStateLabel
            reviewDecision={reviewDecision ?? null}
            state={state}
            variant="small"
          />
        </Box>
        <Box sx={{display: 'inline'}}>
          <Link href={`/${nameWithOwner}/pull/${number}`}>
            #{`${number}`} ({`${nameWithOwner}`}){' '}
            <TrustedRenderedMarkdown trustedHTML={titleHTML} inline={true} />
          </Link>
        </Box>
      </Box>
    );
  });
  return <>{pullRequestList}</>;
}

function ReviewRequestsForUser({
  reviewRequests,
}: {
  reviewRequests: NonNullable<UserHomePageQueryData['search']['nodes']>;
}): React.ReactElement {
  const pullRequests = reviewRequests
    .map(node => (node?.__typename === 'PullRequest' ? node : null))
    .filter(notEmpty);

  return (
    <Box sx={{margin: 20}}>
      <Heading sx={{fontSize: 20, mb: 2}}>
        <Text>Review Requests</Text>
      </Heading>
      {pullRequests.length === 0 ? (
        <Text>No items found.</Text>
      ) : (
        <PullRequests pullRequests={pullRequests} />
      )}
    </Box>
  );
}

function PullRequestsForUser({
  pullRequests,
}: {
  pullRequests: Array<HomePagePullRequestFragment | null>;
}): React.ReactElement {
  return (
    <Box sx={{margin: 20}}>
      <Heading sx={{fontSize: 20, mb: 2}}>
        <Text>Recent Pull Requests</Text>
      </Heading>
      <PullRequests pullRequests={pullRequests} />
    </Box>
  );
}

function RepositoriesForUser({
  repos,
}: {
  repos: Array<{nameWithOwner: string} | null>;
}): React.ReactElement {
  const repoList = repos.map((repo, index) => {
    if (repo != null) {
      const path = `${repo.nameWithOwner}/pulls`;
      return (
        <Box key={index} sx={{pb: 2}}>
          <Link href={`/${path}`}>
            <Text>{path}</Text>
          </Link>
        </Box>
      );
    } else {
      return null;
    }
  });
  return (
    <Box sx={{margin: 20}}>
      <Heading sx={{fontSize: 20, mb: 2}}>
        <Text>Pull Requests for Recent Repositories</Text>
      </Heading>
      {repoList}
    </Box>
  );
}
