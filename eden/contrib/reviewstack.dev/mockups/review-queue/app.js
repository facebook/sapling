const authors = {
  timo: {name: 'Timo Stoffregen', handle: 'TimoStoff', initials: 'TS', avatar: 'avatar-you'},
  alina: {name: 'Alina Petrova', handle: 'alinap', initials: 'AP', avatar: 'avatar-alina'},
  arjun: {name: 'Arjun Mehta', handle: 'arjunm', initials: 'AM', avatar: 'avatar-arjun'},
  maya: {name: 'Maya Chen', handle: 'mayac', initials: 'MC', avatar: 'avatar-maya'},
  jonas: {name: 'Jonas Weber', handle: 'jonasw', initials: 'JW', avatar: 'avatar-jonas'},
  lena: {name: 'Lena Ortiz', handle: 'lenao', initials: 'LO', avatar: 'avatar-lena'},
  sam: {name: 'Sam Rivera', handle: 'samr', initials: 'SR', avatar: 'avatar-sam'},
};

const reviewQueue = [
  {
    number: 142,
    title: 'Stabilize checkpoint resume across multinode runs',
    repo: 'aionic-labs/model_factory',
    author: 'alina',
    state: 'open',
    review: 'Needs review',
    reviewDetail: 'Requested yesterday',
    tone: 'attention',
    updated: '18 min ago',
    updatedAt: '2026-09-16',
    comments: 5,
    stack: 3,
    additions: 328,
    deletions: 91,
  },
  {
    number: 87,
    title: 'Cache sparse temporal features by forecast horizon',
    repo: 'aionic-labs/TimeNet',
    author: 'arjun',
    state: 'open',
    review: 'Changes requested',
    reviewDetail: 'New revision · 2h ago',
    tone: 'blocked',
    updated: '2 hours ago',
    updatedAt: '2026-09-16',
    comments: 12,
    stack: 1,
    additions: 186,
    deletions: 42,
  },
  {
    number: 61,
    title: 'Add encoder registry and pretrained weight resolution',
    repo: 'aionic-labs/model_zoo',
    author: 'maya',
    state: 'open',
    review: 'Needs review',
    reviewDetail: 'Requested 4h ago',
    tone: 'attention',
    updated: '4 hours ago',
    updatedAt: '2026-09-16',
    comments: 3,
    stack: 4,
    additions: 544,
    deletions: 128,
  },
  {
    number: 128,
    title: 'Make dataset manifests deterministic across workers',
    repo: 'aionic-labs/model_factory',
    author: 'jonas',
    state: 'draft',
    review: 'Waiting for author',
    reviewDetail: 'Draft updated today',
    tone: 'neutral',
    updated: '6 hours ago',
    updatedAt: '2026-09-16',
    comments: 8,
    stack: 2,
    additions: 93,
    deletions: 57,
  },
  {
    number: 418,
    title: 'Expose review comments in the ISL comparison view',
    repo: 'aionic-labs/sapling',
    author: 'lena',
    state: 'open',
    review: 'Needs review',
    reviewDetail: 'Requested yesterday',
    tone: 'attention',
    updated: 'Yesterday',
    updatedAt: '2026-09-15',
    comments: 2,
    stack: 1,
    additions: 2911,
    deletions: 12,
  },
  {
    number: 34,
    title: 'Retire the legacy experiment configuration loader',
    repo: 'aionic-labs/platform',
    author: 'sam',
    state: 'merged',
    review: 'Approved',
    reviewDetail: 'Merged this week',
    tone: 'approved',
    updated: 'Monday',
    updatedAt: '2026-09-14',
    comments: 7,
    stack: 1,
    additions: 74,
    deletions: 231,
  },
];

const authoredPullRequests = [
  {
    number: 92,
    title: 'Replace timestamp bucketing with learned temporal bins',
    repo: 'aionic-labs/TimeNet',
    author: 'timo',
    state: 'open',
    review: '2 approvals',
    reviewDetail: 'Ready to land',
    tone: 'approved',
    updated: '11 min ago',
    updatedAt: '2026-09-16',
    comments: 4,
    stack: 3,
    additions: 412,
    deletions: 166,
  },
  {
    number: 151,
    title: 'Route validation artifacts through the shared object store',
    repo: 'aionic-labs/model_factory',
    author: 'timo',
    state: 'open',
    review: 'Changes requested',
    reviewDetail: '1 unresolved thread',
    tone: 'blocked',
    updated: '1 hour ago',
    updatedAt: '2026-09-16',
    comments: 9,
    stack: 2,
    additions: 208,
    deletions: 68,
  },
  {
    number: 66,
    title: 'Publish model cards with evaluation provenance',
    repo: 'aionic-labs/model_zoo',
    author: 'timo',
    state: 'draft',
    review: 'Draft',
    reviewDetail: 'Not requested yet',
    tone: 'neutral',
    updated: '3 hours ago',
    updatedAt: '2026-09-16',
    comments: 0,
    stack: 5,
    additions: 681,
    deletions: 104,
  },
  {
    number: 421,
    title: 'Keep diff tabs open when navigating back to ISL',
    repo: 'aionic-labs/sapling',
    author: 'timo',
    state: 'open',
    review: 'Waiting for review',
    reviewDetail: '2 reviewers requested',
    tone: 'attention',
    updated: 'Yesterday',
    updatedAt: '2026-09-15',
    comments: 1,
    stack: 1,
    additions: 73,
    deletions: 19,
  },
  {
    number: 39,
    title: 'Consolidate environment health checks',
    repo: 'aionic-labs/platform',
    author: 'timo',
    state: 'closed',
    review: 'Closed',
    reviewDetail: 'Superseded by #44',
    tone: 'neutral',
    updated: 'Last week',
    updatedAt: '2026-09-08',
    comments: 6,
    stack: 1,
    additions: 119,
    deletions: 83,
  },
];

const filters = {
  search: document.querySelector('#search-filter'),
  repo: document.querySelector('#repo-filter'),
  author: document.querySelector('#author-filter'),
  status: document.querySelector('#status-filter'),
  dateFrom: document.querySelector('#date-from-filter'),
  dateTo: document.querySelector('#date-to-filter'),
};

const allPullRequests = [...reviewQueue, ...authoredPullRequests];
const view = new URLSearchParams(window.location.search).get('view');

function shortRepo(repo) {
  return repo.split('/')[1];
}

function populateFilters() {
  const repos = [...new Set(allPullRequests.map(pr => pr.repo))].sort();
  const authorKeys = [...new Set(allPullRequests.map(pr => pr.author))].sort((left, right) =>
    authors[left].name.localeCompare(authors[right].name),
  );

  for (const repo of repos) {
    filters.repo.add(new Option(shortRepo(repo), repo));
  }
  for (const authorKey of authorKeys) {
    const label = authorKey === 'timo' ? 'You · Timo Stoffregen' : authors[authorKey].name;
    filters.author.add(new Option(label, authorKey));
  }
}

function matchesFilters(pr) {
  const query = filters.search.value.trim().toLowerCase();
  const searchable = `${pr.title} ${pr.repo} ${pr.number} ${authors[pr.author].name}`.toLowerCase();
  return (
    (query === '' || searchable.includes(query)) &&
    (filters.repo.value === 'all' || filters.repo.value === pr.repo) &&
    (filters.author.value === 'all' || filters.author.value === pr.author) &&
    (filters.status.value === 'all' ||
      (filters.status.value === 'active' && (pr.state === 'open' || pr.state === 'draft')) ||
      filters.status.value === pr.state) &&
    (filters.dateFrom.value === '' || pr.updatedAt >= filters.dateFrom.value) &&
    (filters.dateTo.value === '' || pr.updatedAt <= filters.dateTo.value)
  );
}

function stateLabel(state) {
  return {open: 'Open', draft: 'Draft', merged: 'Merged', closed: 'Closed'}[state];
}

function statusClass(pr) {
  if (pr.state !== 'open') {
    return `status-${pr.state}`;
  }
  return `status-${pr.tone}`;
}

function stackBadge(size) {
  if (size <= 1) {
    return '';
  }
  return `<span class="stack-badge" title="${size} pull requests in this stack">
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="m2 3 4-2 4 2-4 2-4-2Zm0 3 4 2 4-2M2 9l4 2 4-2" /></svg>
    ${size} stack
  </span>`;
}

function rowTemplate(pr) {
  const author = authors[pr.author];
  const status = pr.state === 'open' ? pr.review : stateLabel(pr.state);
  return `<tr class="tone-${pr.tone}">
    <td>
      <a class="pr-title" href="#pr-${pr.number}">${pr.title}</a>
      <div class="pr-meta">
        <span class="repo-name">${shortRepo(pr.repo)}</span>
        <span class="meta-separator">·</span>
        <span>#${pr.number}</span>
        ${stackBadge(pr.stack)}
      </div>
    </td>
    <td class="author-cell-column">
      <div class="author-cell">
        <span class="avatar ${author.avatar}">${author.initials}</span>
        <span class="author-details">
          <span class="author-name">${author.name}</span>
          <span class="author-handle">@${author.handle}</span>
        </span>
      </div>
    </td>
    <td>
      <span class="status-pill ${statusClass(pr)}">${status}</span>
      <span class="review-detail">${pr.reviewDetail}</span>
    </td>
    <td>
      <span class="activity"><strong>${pr.updated}</strong>${pr.comments} comment${
    pr.comments === 1 ? '' : 's'
  }</span>
    </td>
    <td class="changes-cell">
      <span class="change-counts"><span class="additions">+${
        pr.additions
      }</span><span class="deletions">−${pr.deletions}</span></span>
    </td>
  </tr>`;
}

function renderTable(target, pullRequests) {
  if (pullRequests.length === 0) {
    target.innerHTML = `<div class="empty-state"><strong>No pull requests match these filters.</strong><span>Try clearing one or more filters.</span></div>`;
    return;
  }
  target.innerHTML = `<table class="pr-table">
    <thead><tr>
      <th class="title-column">Pull request</th>
      <th class="author-column">Author</th>
      <th class="review-column">Review status</th>
      <th class="activity-column">Updated</th>
      <th class="changes-column">Changes</th>
    </tr></thead>
    <tbody>${pullRequests.map(rowTemplate).join('')}</tbody>
  </table>`;
}

function render() {
  const visibleReviews = reviewQueue.filter(matchesFilters);
  const visibleAuthored = authoredPullRequests.filter(matchesFilters);
  renderTable(document.querySelector('#review-table'), visibleReviews);
  renderTable(document.querySelector('#author-table'), visibleAuthored);
  document.querySelector('#review-count').textContent = visibleReviews.length;
  document.querySelector('#author-count').textContent = visibleAuthored.length;
  document.querySelector('#summary-review-count').textContent = reviewQueue.filter(
    pr => pr.state === 'open' && pr.tone === 'attention',
  ).length;
  document.querySelector('#summary-author-count').textContent = authoredPullRequests.filter(
    pr => pr.state === 'open',
  ).length;
  if (view === 'reviews') {
    document.querySelector('#page-description').textContent = `${
      visibleReviews.length
    } pull request${visibleReviews.length === 1 ? '' : 's'} matching the current filters.`;
  } else if (view === 'authored') {
    document.querySelector('#page-description').textContent = `${
      visibleAuthored.length
    } pull request${visibleAuthored.length === 1 ? '' : 's'} matching the current filters.`;
  }
}

for (const control of Object.values(filters)) {
  control.addEventListener(control === filters.search ? 'input' : 'change', render);
}

document.querySelector('#clear-filters').addEventListener('click', () => {
  filters.search.value = '';
  filters.repo.value = 'all';
  filters.author.value = 'all';
  filters.status.value = 'active';
  filters.dateFrom.value = '';
  filters.dateTo.value = '';
  render();
});

document.addEventListener('keydown', event => {
  if (event.key === '/' && document.activeElement !== filters.search) {
    event.preventDefault();
    filters.search.focus();
  }
});

document.querySelector('#theme-toggle').addEventListener('click', () => {
  const root = document.documentElement;
  root.dataset.theme = root.dataset.theme === 'dark' ? 'light' : 'dark';
});

function configureView() {
  if (view !== 'reviews' && view !== 'authored') {
    return;
  }
  document.body.classList.add('detail-mode');
  document.querySelector('#back-link').classList.add('visible');
  document.querySelector('#page-eyebrow').textContent = 'Focused queue';
  document.querySelector('.summary-strip').hidden = true;
  if (view === 'reviews') {
    document.title = 'Needs your review · ReviewStack mock-up';
    document.querySelector('#page-title').textContent = 'Needs your review';
    document.querySelector('#author-section').hidden = true;
  } else {
    document.title = 'Your pull requests · ReviewStack mock-up';
    document.querySelector('#page-title').textContent = 'Your pull requests';
    document.querySelector('#review-section').hidden = true;
  }
}

configureView();
populateFilters();
render();
