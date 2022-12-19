# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import hashlib
from abc import abstractmethod
from typing import Any, Callable, Dict, List, Optional, Union

from edenscm.ext.github.consts import query

from ghstack.github_gh_cli import Result

from .consts import GITHUB_HOSTNAME

"""utils for mocking GitHub requests.

This is defined to be used with `extensions.wrapfunction` to replace the real
GitHub requests in tests.
"""

OWNER = "facebook"
OWNER_ID = "facebook_id"
REPO_NAME = "test_github_repo"
REPO_ID = "R_test_github_repo"
USER_NAME = "facebook_username"

ParamsType = Dict[str, Union[bool, int, str]]
MakeRequestType = Callable[[ParamsType, str, str, Optional[str]], Result]
RunGitCommandType = Callable[[List[str], str], bytes]


# TODO:
# 1. verify the arguments of the git push commands
def mock_run_git_command(
    origin_run_git_command: RunGitCommandType,
    args: List[str],
    gitdir: str,
) -> bytes:
    """Wrapper function for `run_git_command`.

    For 'git push' operation, it just returns without sending a network request;
    otherwise call the real function.
    """
    if args and args[0] == "push":
        return b""
    return origin_run_git_command(args, gitdir)


# TODO:
# 1. a way to verify all of the expected mocks in MockGitHubServer were called.
#      * one way to do this is logging each call to a file
# 2. for multiple commands, we need to create a config extension for each command.
#    Should we maintain a state between commands?
# 3. improve error message, when a request_key not found in the server mock data,
#    try to find the closest key and show the diff.
class MockGitHubServer:
    """A mock GitHub server for replacing `github_gh_cli.make_request`.

    The internal `requests` is a mapping of request_keys and mock requests. .

    Example usage::

      github_server = MockGithubServer()
      github_server.expect_get_repository_request().and_respond()

      extensions.wrapfunction(
        github_gh_cli, "make_request", github_server.make_request
      )
    """

    def __init__(self, hostname: str = GITHUB_HOSTNAME):
        self.hostname: str = hostname
        self.requests: Dict[str, MockRequest] = {}

    async def make_request(
        self,
        real_make_request: MakeRequestType,
        params: ParamsType,
        hostname: str,
        endpoint: str = "graphql",
        method: Optional[str] = None,
    ) -> Result:
        """Wrapper function for `github_gh_cli.make_request`.

        It reads mock data from `self.requests` instead of sending network requests.
        """
        assert (
            real_make_request.__name__ == "make_request"
        ), f"expected 'make_request', but got '{real_make_request.__name__}'"

        key = create_request_key(params, hostname, endpoint, method)

        try:
            return self.requests[key].get_response()
        except KeyError:
            raise MockRequestNotFound(key)

    def _add_request(self, request_key: str, request: "MockRequest") -> None:
        self.requests[request_key] = request

    def expect_get_repository_request(
        self, owner: str = OWNER, name: str = REPO_NAME
    ) -> "GetRepositoryRequest":
        params: ParamsType = {
            "query": query.GRAPHQL_GET_REPOSITORY,
            "owner": owner,
            "name": name,
        }
        key = create_request_key(params, self.hostname)
        request = GetRepositoryRequest(key, owner, name)
        self._add_request(key, request)
        return request

    def expect_create_pr_placeholder_request(
        self, owner: str = OWNER, name: str = REPO_NAME
    ) -> "CreatePrPlaceholderRequest":
        endpoint = f"repos/{owner}/{name}/issues"
        params: ParamsType = {
            "title": "placeholder for pull request",
        }
        key = create_request_key(params, self.hostname, endpoint=endpoint)
        request = CreatePrPlaceholderRequest(key)
        self._add_request(key, request)
        return request

    def expect_create_pr_request(
        self,
        body: str,
        issue: int,
        head: str = "",
        is_draft: bool = False,
        base: str = "main",
        owner: str = OWNER,
        name: str = REPO_NAME,
    ) -> "CreatePrRequest":
        params: ParamsType = {
            "base": base,
            "head": head or f"pr{issue}",
            "body": body,
            "issue": issue,
            "draft": is_draft,
        }
        endpoint = f"repos/{owner}/{name}/pulls"
        key = create_request_key(params, self.hostname, endpoint=endpoint)
        request = CreatePrRequest(key, owner, name, issue)
        self._add_request(key, request)
        return request


class MockRequest:
    @abstractmethod
    def get_response(self) -> Result:
        pass


class GetRepositoryRequest(MockRequest):
    def __init__(self, key: str, owner: str, name: str) -> None:
        self._key = key
        self._response: Optional[Result] = None

        self._owner = owner
        self._name = name

    def and_respond(
        self,
        repo_id: str = REPO_ID,
        owner_id: str = OWNER_ID,
        is_fork: bool = False,
        default_branch_ref: str = "main",
        parent: Optional[Any] = None,
    ):
        data = {
            "data": {
                "repository": {
                    "id": repo_id,
                    "owner": {"id": owner_id, "login": self._owner},
                    "name": self._name,
                    "isFork": is_fork,
                    "defaultBranchRef": {"name": default_branch_ref},
                    "parent": parent,
                }
            }
        }
        self._response = Result.Ok(data)

    def get_response(self) -> Result:
        if self._response is None:
            raise MockResponseNotSet(self._key)
        return self._response


class CreatePrPlaceholderRequest(MockRequest):
    def __init__(self, key: str) -> None:
        self._key = key

        self._current_number = 1
        self._num_times = 1

    def and_respond(self, start_number: int = 1, num_times: int = 1) -> None:
        self._current_number = start_number
        self._num_times = num_times

    def get_response(self) -> Result:
        if self._num_times > 0:
            number = self._current_number
            self._current_number += 1
            self._num_times -= 1
            return Result.Ok({"number": number})
        else:
            raise MockResponseRunout(self._key)


class CreatePrRequest(MockRequest):
    def __init__(self, key: str, owner, name: str, number: int) -> None:
        self._key: str = key
        self._response: Optional[Result] = None

        self._owner = owner
        self._name = name
        self._number = number

    def and_respond(self):
        self._response = Result.Ok(
            {
                "number": self._number,
                "html_url": f"https://github.com/{self._owner}/{self._name}/pull/{self._number}",
            }
        )

    def get_response(self) -> Result:
        if self._response is None:
            raise MockResponseNotSet(self._key)
        return self._response


class MockRequestNotFound(Exception):
    pass


class MockResponseNotSet(Exception):
    pass


class MockResponseRunout(Exception):
    pass


def create_request_key(
    params: Dict[str, Union[str, int, bool]],
    hostname: str = GITHUB_HOSTNAME,
    endpoint: str = "graphql",
    method: Optional[str] = None,
) -> str:
    """Create a string key from the input of `make_request` function.

    This will be used to verify the input and find corresponding output.
    """
    s = ",".join(f"{k}={v}" for k, v in sorted(params.items()))
    return f"{hostname}|{endpoint}|{method}|{s}"


def gen_hash_hexdigest(s: str) -> str:
    """generate sha1 digit hex string for input `s`"""
    return hashlib.sha1(s.encode()).hexdigest()
