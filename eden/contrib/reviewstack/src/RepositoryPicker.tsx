import URLFor from './URLFor';
import {
  type AccessibleRepository,
  fetchAccessibleRepositories,
} from './github/repositories';
import useNavigate from './useNavigate';
import {ArrowRightIcon, StackIcon} from '@primer/octicons-react';
import React, {useEffect, useState, type FormEvent} from 'react';

import './RepositoryPicker.css';

export function parseRepositoryInput(value: string): {org: string; repo: string} | null {
  let path = value.trim();
  if (path === '') {
    return null;
  }

  if (path.includes('://') || path.startsWith('github.com/')) {
    try {
      const url = new URL(path.includes('://') ? path : `https://${path}`);
      if (url.hostname !== 'github.com' && url.hostname !== 'www.github.com') {
        return null;
      }
      path = url.pathname;
    } catch {
      return null;
    }
  }

  const parts = path.split('/').filter(Boolean);
  if (parts.length !== 2 || parts.some(part => part.includes('.'))) {
    return null;
  }
  return {org: parts[0], repo: parts[1]};
}

export default function RepositoryPicker(): React.ReactElement {
  const navigate = useNavigate();
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [repositories, setRepositories] = useState<AccessibleRepository[] | null>(null);
  const [repositoryFilter, setRepositoryFilter] = useState('');

  useEffect(() => {
    const token = localStorage.getItem('github.token');
    if (token == null) {
      return;
    }

    const hostname = localStorage.getItem('github.hostname') ?? 'github.com';
    let cancelled = false;
    fetchAccessibleRepositories(hostname, token)
      .then(result => {
        if (!cancelled) {
          setRepositories(result);
        }
      })
      .catch(fetchError => {
        if (!cancelled) {
          setError(fetchError instanceof Error ? fetchError.message : 'Unable to load repositories.');
          setRepositories([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const visibleRepositories =
    repositories?.filter(repository =>
      repository.fullName.toLowerCase().includes(repositoryFilter.toLowerCase().trim()),
    ) ?? [];

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const repository = parseRepositoryInput(value);
    if (repository == null) {
      setError('Enter a repository as owner/name or a GitHub repository URL.');
      return;
    }
    setError(null);
    navigate(URLFor.project(repository));
  };

  return (
    <main className="reviewstack-landing">
      <section className="reviewstack-landing-panel" aria-labelledby="repository-picker-title">
        <div className="reviewstack-landing-mark" aria-hidden="true">
          <StackIcon size={30} />
        </div>
        <p className="reviewstack-landing-eyebrow">ReviewStack</p>
        <h1 id="repository-picker-title">Choose a repository</h1>
        <p className="reviewstack-landing-description">
          Select a repository you can access, or enter one directly to open its pull-request queue.
        </p>
        {repositories == null ? (
          <p className="reviewstack-repository-status">Loading your repositories...</p>
        ) : repositories.length > 0 ? (
          <section className="reviewstack-repository-list" aria-labelledby="repository-list-title">
            <div className="reviewstack-repository-list-header">
              <h2 id="repository-list-title">Your repositories</h2>
              <span>{repositories.length}</span>
            </div>
            <input
              className="reviewstack-repository-filter"
              type="search"
              placeholder="Filter repositories"
              value={repositoryFilter}
              onChange={event => setRepositoryFilter(event.target.value)}
              aria-label="Filter repositories"
            />
            <div className="reviewstack-repository-options">
              {visibleRepositories.map(repository => (
                <button
                  className="reviewstack-repository-option"
                  key={repository.fullName}
                  type="button"
                  onClick={() => navigate(URLFor.project(parseRepositoryInput(repository.fullName)!))}>
                  <span>{repository.fullName}</span>
                  {repository.private && <small>Private</small>}
                </button>
              ))}
              {visibleRepositories.length === 0 && (
                <p className="reviewstack-repository-empty">No repositories match that filter.</p>
              )}
            </div>
          </section>
        ) : null}
        <div className="reviewstack-manual-repository">
          <h2>Open by name</h2>
        <form onSubmit={onSubmit} noValidate>
          <label htmlFor="repository-input">GitHub repository</label>
          <div className="reviewstack-landing-input-row">
            <input
              id="repository-input"
              type="text"
              autoComplete="off"
              autoFocus
              placeholder="owner/repository"
              value={value}
              onChange={event => {
                setValue(event.target.value);
                if (error != null) {
                  setError(null);
                }
              }}
              aria-invalid={error != null}
              aria-describedby={error != null ? 'repository-input-error' : undefined}
            />
            <button className="reviewstack-open-button" type="submit">
              Open repository
              <ArrowRightIcon size={16} aria-hidden="true" />
            </button>
          </div>
          {error != null && (
            <p className="reviewstack-landing-error" id="repository-input-error" role="alert">
              {error}
            </p>
          )}
        </form>
        </div>
      </section>
    </main>
  );
}