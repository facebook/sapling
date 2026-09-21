/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {HomePagePullRequestFragment} from './generated/graphql';
import type {GitHubUserHomePageData} from './jotai/atoms';

import ActorAvatar from './ActorAvatar';
import CenteredSpinner from './CenteredSpinner';
import Link from './Link';
import {PullRequestReviewDecision, PullRequestState} from './generated/graphql';
import {GitHubGraphQLError} from './github/queryGraphQL';
import {gitHubUserHomePageDataAtom} from './jotai/atoms';
import {SearchIcon} from '@primer/octicons-react';
import {Box, Button, Flash, Text, useTheme} from '@primer/react';
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import React, {useMemo, useState} from 'react';
import {notEmpty} from 'shared/utils';

import './UserHomePage.css';

type PullRequest = HomePagePullRequestFragment;
type QueueView = 'reviews' | 'authored' | null;
type Filters = {
  search: string;
  organization: string;
  repo: string;
  author: string;
  status: string;
  dateFrom: string;
  dateTo: string;
};

const DEFAULT_FILTERS: Filters = {
  search: '',
  organization: 'all',
  repo: 'all',
  author: 'all',
  status: 'active',
  dateFrom: '',
  dateTo: '',
};

const loadableGitHubUserHomePageDataAtom = loadable(gitHubUserHomePageDataAtom);

export default function UserHomePage(): React.ReactElement {
  const result = useAtomValue(loadableGitHubUserHomePageDataAtom);
  switch (result.state) {
    case 'loading':
      return <CenteredSpinner />;
    case 'hasError':
      if (result.error instanceof GitHubGraphQLError && result.error.isRateLimitError) {
        return <RateLimitNotice error={result.error} />;
      }
      throw result.error;
    case 'hasData':
      return <UserHomePageRoot data={result.data} />;
  }
}

function RateLimitNotice({error}: {error: GitHubGraphQLError}): React.ReactElement {
  const resetTime =
    error.rateLimitReset == null
      ? null
      : new Date(error.rateLimitReset * 1000).toLocaleTimeString([], {
          hour: '2-digit',
          minute: '2-digit',
          timeZoneName: 'short',
        });
  return (
    <Box sx={{maxWidth: 720, margin: '40px auto', padding: '0 24px'}}>
      <Flash variant="warning">
        <Text as="h1" sx={{display: 'block', fontSize: 2, fontWeight: 'bold', marginBottom: 2}}>
          GitHub API limit reached
        </Text>
        <Text as="p" sx={{display: 'block', margin: 0}}>
          GitHub is temporarily rejecting GraphQL requests for your account.
          {resetTime == null ? '' : ` GitHub says the limit resets at ${resetTime}.`}
        </Text>
        <Button sx={{marginTop: 3}} onClick={() => window.location.reload()}>
          Try again
        </Button>
      </Flash>
    </Box>
  );
}

function UserHomePageRoot({data}: {data: GitHubUserHomePageData | null}): React.ReactElement {
  const reviewRequests = useMemo(() => extractReviewRequests(data), [data]);
  const authoredPullRequests = useMemo(() => data?.pullRequests.filter(notEmpty) ?? [], [data]);
  const [filters, setFilters] = useState(DEFAULT_FILTERS);
  const view = getQueueView();
  const filteredReviews = reviewRequests.filter(pullRequest =>
    matchesFilters(pullRequest, filters),
  );
  const filteredAuthored = authoredPullRequests.filter(pullRequest =>
    matchesFilters(pullRequest, filters),
  );
  const repositories = uniqueValues(
    [...reviewRequests, ...authoredPullRequests].map(
      pullRequest => pullRequest.repository.nameWithOwner,
    ),
  );
  const organizations = uniqueValues(repositories.map(repositoryOrganization));
  const authors = uniqueValues(
    [...reviewRequests, ...authoredPullRequests]
      .map(pullRequest => pullRequest.author?.login)
      .filter(notEmpty),
  );
  const visibleCount = view === 'reviews' ? filteredReviews.length : filteredAuthored.length;
  const {theme} = useTheme();
  const colors = theme?.colors;
  const themeVariables = {
    '--queue-bg': colors?.canvas.default,
    '--queue-surface': colors?.canvas.overlay,
    '--queue-subtle': colors?.canvas.subtle,
    '--queue-border': colors?.border.default,
    '--queue-border-muted': colors?.border.muted,
    '--queue-text': colors?.fg.default,
    '--queue-muted': colors?.fg.muted,
    '--queue-accent': colors?.accent.fg,
    '--queue-accent-muted': colors?.accent.muted,
    '--queue-attention': colors?.attention.fg,
    '--queue-attention-muted': colors?.attention.muted,
    '--queue-danger': colors?.danger.fg,
    '--queue-danger-muted': colors?.danger.muted,
    '--queue-success': colors?.success.fg,
    '--queue-success-muted': colors?.success.muted,
    '--queue-done': colors?.done.fg,
    '--queue-done-muted': colors?.done.muted,
  } as React.CSSProperties;

  return (
    <main
      className={`review-queue-page${view == null ? '' : ' review-queue-page-focused'}`}
      style={themeVariables}>
      <section className="review-queue-heading">
        <div>
          {view != null && (
            <a className="review-queue-back" href="/">
              ← Review work
            </a>
          )}
          <p className="review-queue-eyebrow">
            {view == null ? 'ReviewStack home' : 'Focused queue'}
          </p>
          <h1>
            {view === 'reviews'
              ? 'Needs your review'
              : view === 'authored'
              ? 'Your pull requests'
              : 'Review work'}
          </h1>
          <p className="review-queue-description">
            {view == null
              ? 'Everything waiting for you, across all repositories.'
              : `${visibleCount} pull request${
                  visibleCount === 1 ? '' : 's'
                } matching the current filters.`}
          </p>
        </div>
        {view == null && (
          <div className="review-queue-summary" aria-label="Review summary">
            <a
              className="review-queue-summary-item review-queue-summary-attention"
              href="/?view=reviews">
              <strong>{reviewRequests.filter(isActive).length}</strong>
              <span>Need your review</span>
            </a>
            <a className="review-queue-summary-item" href="/?view=authored">
              <strong>{authoredPullRequests.filter(isActive).length}</strong>
              <span>Open by you</span>
            </a>
          </div>
        )}
      </section>

      <FilterBar
        filters={filters}
        organizations={organizations}
        repositories={repositories}
        authors={authors}
        onChange={setFilters}
      />

      {view !== 'authored' && (
        <QueueSection
          id="review-requests"
          title="Needs your review"
          description="Pull requests where you are a requested reviewer."
          href="/?view=reviews"
          pullRequests={filteredReviews}
          focused={view === 'reviews'}
        />
      )}
      {view !== 'reviews' && (
        <QueueSection
          id="authored-pull-requests"
          title="Your pull requests"
          description="Changes you authored that still need attention."
          href="/?view=authored"
          pullRequests={filteredAuthored}
          focused={view === 'authored'}
        />
      )}
    </main>
  );
}

function FilterBar({
  filters,
  organizations,
  repositories,
  authors,
  onChange,
}: {
  filters: Filters;
  organizations: string[];
  repositories: string[];
  authors: string[];
  onChange: (filters: Filters) => void;
}): React.ReactElement {
  const update = (field: keyof Filters, value: string) => onChange({...filters, [field]: value});
  const updateOrganization = (organization: string) =>
    onChange({...filters, organization, repo: 'all'});
  const visibleRepositories = repositories.filter(
    repository =>
      filters.organization === 'all' ||
      repositoryOrganization(repository) === filters.organization,
  );
  return (
    <section className="review-queue-filters" aria-label="Pull request filters">
      <label className="review-queue-search">
        <span className="review-queue-sr-only">Filter by title or number</span>
        <SearchIcon size={16} aria-hidden="true" />
        <input
          type="search"
          placeholder="Filter by title or number…"
          value={filters.search}
          onChange={event => update('search', event.target.value)}
        />
      </label>
      <FilterSelect
        label="Organization"
        value={filters.organization}
        onChange={updateOrganization}>
        <option value="all">All organizations</option>
        {organizations.map(organization => (
          <option value={organization} key={organization}>
            {organization}
          </option>
        ))}
      </FilterSelect>
      <FilterSelect
        label="Repository"
        value={filters.repo}
        onChange={value => update('repo', value)}>
        <option value="all">All repositories</option>
        {visibleRepositories.map(repository => (
          <option value={repository} key={repository}>
            {shortRepository(repository)}
          </option>
        ))}
      </FilterSelect>
      <FilterSelect
        label="Author"
        value={filters.author}
        onChange={value => update('author', value)}>
        <option value="all">All authors</option>
        {authors.map(author => (
          <option value={author} key={author}>
            {author}
          </option>
        ))}
      </FilterSelect>
      <FilterSelect
        label="PR status"
        value={filters.status}
        onChange={value => update('status', value)}>
        <option value="active">Open and draft</option>
        <option value="open">Open</option>
        <option value="draft">Draft</option>
        <option value="merged">Merged</option>
        <option value="closed">Closed</option>
        <option value="all">All statuses</option>
      </FilterSelect>
      <div className="review-queue-filter-control review-queue-date-filter">
        <span>Date Filter</span>
        <div className="review-queue-date-inputs">
          <label>
            <span className="review-queue-sr-only">Updated from</span>
            <input
              type="date"
              aria-label="Updated from"
              value={filters.dateFrom}
              onChange={event => update('dateFrom', event.target.value)}
            />
          </label>
          <span>to</span>
          <label>
            <span className="review-queue-sr-only">Updated to</span>
            <input
              type="date"
              aria-label="Updated to"
              value={filters.dateTo}
              onChange={event => update('dateTo', event.target.value)}
            />
          </label>
        </div>
      </div>
      <button
        className="review-queue-clear"
        type="button"
        onClick={() => onChange(DEFAULT_FILTERS)}>
        Clear filters
      </button>
    </section>
  );
}

function FilterSelect({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <label className="review-queue-filter-control">
      <span>{label}</span>
      <select value={value} onChange={event => onChange(event.target.value)}>
        {children}
      </select>
    </label>
  );
}

function QueueSection({
  id,
  title,
  description,
  href,
  pullRequests,
  focused,
}: {
  id: string;
  title: string;
  description: string;
  href: string;
  pullRequests: PullRequest[];
  focused: boolean;
}): React.ReactElement {
  return (
    <section id={id} className="review-queue-section" aria-labelledby={`${id}-title`}>
      {!focused && (
        <div className="review-queue-section-heading">
          <div>
            <div className="review-queue-title-line">
              <h2 id={`${id}-title`}>
                <a href={href}>{title}</a>
              </h2>
              <span className="review-queue-count">{pullRequests.length}</span>
            </div>
            <p>{description}</p>
          </div>
        </div>
      )}
      <PullRequestTable pullRequests={pullRequests} />
    </section>
  );
}

function PullRequestTable({pullRequests}: {pullRequests: PullRequest[]}): React.ReactElement {
  if (pullRequests.length === 0) {
    return (
      <div className="review-queue-table-card review-queue-empty">
        <strong>No pull requests match these filters.</strong>
        <span>Try clearing one or more filters.</span>
      </div>
    );
  }
  return (
    <div className="review-queue-table-card">
      <table className="review-queue-table">
        <thead>
          <tr>
            <th>Pull request</th>
            <th className="review-queue-author-column">Author</th>
            <th className="review-queue-status-column">Review status</th>
            <th className="review-queue-activity-column">Updated</th>
            <th className="review-queue-changes-column">Changes</th>
          </tr>
        </thead>
        <tbody>
          {pullRequests.map(pullRequest => (
            <PullRequestRow
              key={`${pullRequest.repository.nameWithOwner}-${pullRequest.number}`}
              pullRequest={pullRequest}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PullRequestRow({pullRequest}: {pullRequest: PullRequest}): React.ReactElement {
  const {author, comments, number, repository, title, updatedAt} = pullRequest;
  const status = getStatus(pullRequest);
  return (
    <tr className={`review-queue-tone-${status.tone}`}>
      <td>
        <Link href={`/${repository.nameWithOwner}/pull/${number}`}>{title}</Link>
        <div className="review-queue-meta">
          <span>{shortRepository(repository.nameWithOwner)}</span>
          <span>·</span>
          <span>#{number}</span>
        </div>
      </td>
      <td className="review-queue-author-column">
        <div className="review-queue-author">
          <ActorAvatar login={author?.login} url={author?.avatarUrl} size={28} />
          <span>{author?.login ?? 'ghost'}</span>
        </div>
      </td>
      <td className="review-queue-status-column">
        <span className={`review-queue-status review-queue-status-${status.tone}`}>
          {status.label}
        </span>
        <span className="review-queue-status-detail">{status.detail}</span>
      </td>
      <td className="review-queue-activity-column">
        <strong>{relativeDate(updatedAt)}</strong>
        <span>
          {comments.totalCount} comment{comments.totalCount === 1 ? '' : 's'}
        </span>
      </td>
      <td className="review-queue-changes-column">
        <span className="review-queue-additions">+{pullRequest.additions}</span>{' '}
        <span className="review-queue-deletions">−{pullRequest.deletions}</span>
      </td>
    </tr>
  );
}

function extractReviewRequests(data: GitHubUserHomePageData | null): PullRequest[] {
  return (data?.reviewRequests ?? [])
    .map(node => (node?.__typename === 'PullRequest' ? node : null))
    .filter(notEmpty)
    .filter(needsReview);
}

function needsReview(pullRequest: PullRequest): boolean {
  return (
    pullRequest.reviewDecision !== PullRequestReviewDecision.Approved &&
    pullRequest.reviewDecision !== PullRequestReviewDecision.ChangesRequested
  );
}

function getQueueView(): QueueView {
  const view = new URLSearchParams(window.location.search).get('view');
  return view === 'reviews' || view === 'authored' ? view : null;
}

function matchesFilters(pullRequest: PullRequest, filters: Filters): boolean {
  const query = filters.search.trim().toLowerCase();
  const searchable = `${pullRequest.title} ${pullRequest.repository.nameWithOwner} ${
    pullRequest.number
  } ${pullRequest.author?.login ?? ''}`.toLowerCase();
  const updatedDate = pullRequest.updatedAt.slice(0, 10);
  return (
    (query === '' || searchable.includes(query)) &&
    (filters.organization === 'all' ||
      filters.organization === repositoryOrganization(pullRequest.repository.nameWithOwner)) &&
    (filters.repo === 'all' || filters.repo === pullRequest.repository.nameWithOwner) &&
    (filters.author === 'all' || filters.author === pullRequest.author?.login) &&
    (filters.status === 'all' ||
      (filters.status === 'active' && isActive(pullRequest)) ||
      filters.status === pullRequestStatus(pullRequest)) &&
    (filters.dateFrom === '' || updatedDate >= filters.dateFrom) &&
    (filters.dateTo === '' || updatedDate <= filters.dateTo)
  );
}

function pullRequestStatus(pullRequest: PullRequest): string {
  if (pullRequest.state === PullRequestState.Merged) {
    return 'merged';
  }
  if (pullRequest.state === PullRequestState.Closed) {
    return 'closed';
  }
  return pullRequest.isDraft ? 'draft' : 'open';
}

function isActive(pullRequest: PullRequest): boolean {
  return pullRequest.state === PullRequestState.Open;
}

function getStatus(pullRequest: PullRequest): {label: string; detail: string; tone: string} {
  if (pullRequest.state === PullRequestState.Merged) {
    return {label: 'Merged', detail: 'Completed', tone: 'done'};
  }
  if (pullRequest.state === PullRequestState.Closed) {
    return {label: 'Closed', detail: 'Closed without merging', tone: 'danger'};
  }
  if (pullRequest.isDraft) {
    return {label: 'Draft', detail: 'Not ready for review', tone: 'neutral'};
  }
  switch (pullRequest.reviewDecision) {
    case PullRequestReviewDecision.Approved:
      return {label: 'Approved', detail: 'Ready to land', tone: 'success'};
    case PullRequestReviewDecision.ChangesRequested:
      return {label: 'Changes requested', detail: 'Author follow-up needed', tone: 'danger'};
    case PullRequestReviewDecision.ReviewRequired:
      return {label: 'Needs review', detail: 'Review requested', tone: 'attention'};
    default:
      return {label: 'Waiting for review', detail: 'No decision yet', tone: 'attention'};
  }
}

function relativeDate(isoDate: string): string {
  const milliseconds = Date.now() - new Date(isoDate).getTime();
  const minutes = Math.max(0, Math.floor(milliseconds / 60_000));
  if (minutes < 60) {
    return minutes <= 1 ? 'Just now' : `${minutes} min ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours} hour${hours === 1 ? '' : 's'} ago`;
  }
  const days = Math.floor(hours / 24);
  if (days < 7) {
    return days === 1 ? 'Yesterday' : `${days} days ago`;
  }
  return new Date(isoDate).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

function shortRepository(repository: string): string {
  return repository.split('/').at(-1) ?? repository;
}

function repositoryOrganization(repository: string): string {
  return repository.split('/')[0] ?? repository;
}

function uniqueValues(values: string[]): string[] {
  return [...new Set(values)].sort((left, right) => left.localeCompare(right));
}
