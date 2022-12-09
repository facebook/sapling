from abc import ABCMeta, abstractmethod
from typing import Any, Sequence


class GitHubEndpoint(metaclass=ABCMeta):
    @abstractmethod
    async def graphql(self, query: str, **kwargs: Any) -> Any:
        """Execute a GraphQL query asynchronously.

        Args:
            query: string GraphQL query to execute
            **kwargs: values for variables in the graphql query

        Returns: parsed JSON response
        """
        pass

    @abstractmethod
    def graphql_sync(self, query: str, **kwargs: Any) -> Any:
        """Execute a GraphQL query synchronously.

        Args:
            query: string GraphQL query to execute
            **kwargs: values for variables in the graphql query

        Returns: parsed JSON response
        """
        pass

    # This hook function should be invoked when a 'git push' to GitHub
    # occurs.  This is used by testing to simulate actions GitHub
    # takes upon branch push, more conveniently than setting up
    # a branch hook on the repository and receiving events from it.
    # TODO: generalize to any repo
    @abstractmethod
    def push_hook(self, refName: Sequence[str]) -> None:
        pass

    def get(self, path: str, **kwargs: Any) -> Any:
        """
        Send a GET request to endpoint 'path'.

        Returns: parsed JSON response
        """
        return self.rest('get', path, **kwargs)

    def post(self, path: str, **kwargs: Any) -> Any:
        """
        Send a POST request to endpoint 'path'.

        Returns: parsed JSON response
        """
        return self.rest('post', path, **kwargs)

    def patch(self, path: str, **kwargs: Any) -> Any:
        """
        Send a PATCH request to endpoint 'path'.

        Returns: parsed JSON response
        """
        return self.rest('patch', path, **kwargs)

    @abstractmethod
    def rest(self, method: str, path: str, **kwargs: Any) -> Any:
        """
        Send a 'method' request to endpoint 'path'.

        Args:
            method: 'GET', 'POST', etc.
            path: relative URL path to access on endpoint
            **kwargs: dictionary of JSON payload to send

        Returns: parsed JSON response
        """
        pass


def get_github_endpoint(hostname: str) -> GitHubEndpoint:
    """Factory method to generate a GitHubEndpoint.

    Currently, it only supports GitHubCLIEndpoint. We will add a fake
    GitHubEndpoint for integration test later.
    """
    from ghstack.github_cli_endpoint import GitHubCLIEndpoint
    return GitHubCLIEndpoint(hostname)
