import {createRequestHeaders} from 'shared/github/auth';

export type AccessibleRepository = {
  name: string;
  fullName: string;
  private: boolean;
};

function createRepositoriesEndpoint(hostname: string, page: number): string {
  const base =
    hostname === 'github.com' ? 'https://api.github.com' : `https://${hostname}/api/v3`;
  return `${base}/user/repos?affiliation=owner,collaborator,organization_member&per_page=100&sort=updated&page=${page}`;
}

export function fetchAccessibleRepositories(
  hostname: string,
  token: string,
): Promise<AccessibleRepository[]> {
  async function fetchPage(page: number): Promise<AccessibleRepository[]> {
    const response = await fetch(createRepositoriesEndpoint(hostname, page), {
      headers: createRequestHeaders(token),
    });
    if (!response.ok) {
      throw new Error(`GitHub repository request failed: ${response.status}`);
    }

    const pageRepositories = (await response.json()) as Array<{
      name?: string;
      full_name?: string;
      private?: boolean;
    }>;
    if (!Array.isArray(pageRepositories)) {
      throw new Error('GitHub returned an invalid repository list.');
    }

    const repositories = pageRepositories.flatMap(repository =>
      repository.name != null && repository.full_name != null
        ? [
            {
              name: repository.name,
              fullName: repository.full_name,
              private: repository.private === true,
            },
          ]
        : [],
    );
    return pageRepositories.length === 100 && page < 10
      ? repositories.concat(await fetchPage(page + 1))
      : repositories;
  }

  return fetchPage(1);
}