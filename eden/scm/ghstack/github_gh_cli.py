# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import itertools
import json
from dataclasses import dataclass
from typing import Any, Dict, Generic, List, Optional, Sequence, TypeVar, Union

import ghstack.github

from edenscm import error
from edenscm.i18n import _


class GitHubCLIEndpoint(ghstack.github.GitHubEndpoint):
    """Alternative to RealGitHubEndpoint that makes all of its requests via the
    GitHub CLI. The primary benefit to end-users is that there is no need to
    create a ~/.ghstackrc file, which can be a stumbling block when getting
    started with ghstack.

    Though note the primary tradeoff is that invoking a method of this class
    entails spawning a new process, which may be problematic for Windows users.
    """

    def __init__(self):
        pass

    def push_hook(self, refName: Sequence[str]) -> None:
        pass

    def rest(self, method: str, path: str, **kwargs: Any) -> Any:
        params: Dict[str, Union[str, int, bool]] = dict(kwargs)
        loop = asyncio.get_event_loop()
        result = loop.run_until_complete(
            make_request(params, endpoint=path, method=method)
        )
        if result.is_error():
            raise RuntimeError(result.error)
        else:
            return result.ok

    def graphql(self, query: str, **kwargs: Any) -> Any:
        params: Dict[str, Union[str, int, bool]] = dict(kwargs)
        params["query"] = query
        loop = asyncio.get_event_loop()
        result = loop.run_until_complete(make_request(params))
        if result.is_error():
            raise RuntimeError(result.error)
        else:
            return result.ok


"""
TODO: Unify this code with the original in eden/scm/edenscm/ext/github/gh_submit.py.
Note that this is designed to be used with async/await so that commands can be
run in parallel, where possible, though GitHubEndpoint currently has a
synchronous API, so a bit of cleanup needs to be done to take advantage of
concurrency.
"""


T = TypeVar("T")


@dataclass
class Result(Generic[T]):
    ok: Optional[T] = None
    error: Optional[str] = None

    def is_error(self) -> bool:
        return self.error is not None


async def make_request(
    params: Dict[str, Union[str, int, bool]],
    endpoint="graphql",
    method: Optional[str] = None,
) -> Result:
    """If successful, returns a Result whose value is parsed JSON returned by
    the request.
    """
    if method:
        endpoint_args = ["-X", method.upper(), endpoint]
    else:
        endpoint_args = [endpoint]
    args = (
        ["gh", "api"]
        + endpoint_args
        + list(itertools.chain(*[format_param(k, v) for (k, v) in params.items()]))
    )
    proc = await asyncio.create_subprocess_exec(
        *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
    )
    stdout, stderr = await proc.communicate()

    # If proc exits with a non-zero exit code, the stdout may still
    # be valid JSON, but we expect it to have an "errors" property defined.
    try:
        response = json.loads(stdout)
    except json.JSONDecodeError:
        response = None

    if proc.returncode == 0:
        assert response is not None
        assert "errors" not in response
        return Result(ok=response)
    elif response is not None:
        return Result(error=json.dumps(response, indent=1))
    elif b"gh auth login" in stderr:
        # The error message is likely referring to an authentication issue.
        raise error.Abort(_("Error calling the GitHub API:\n%s") % stderr.decode())
    else:
        return Result(
            error=f"exit({proc.returncode}) Failure running {' '.join(args)}\nstdout: {stdout.decode()}\nstderr: {stderr.decode()}\n"
        )


def format_param(key: str, value: Union[str, int, bool]) -> List[str]:
    # In Python, bool is a subclass of int, so check it first.
    if isinstance(value, bool):
        opt = "-F"
        val = str(value).lower()
    elif isinstance(value, int):
        opt = "-F"
        val = value
    elif isinstance(value, str):
        opt = "-f"
        val = str(value)
    else:
        raise RuntimeError(f"unexpected param: {key}={value}")
    return [opt, f"{key}={val}"]
