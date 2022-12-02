# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""templates to use when the GitHub extension is enabled
"""

from typing import Optional

from .pr_parser import get_pull_request_for_context
from .pr_status import get_pull_request_data
from .pullrequest import GraphQLPullRequest, PullRequestId
from .pullrequeststore import PullRequestStore


def github_pull_request_repo_owner(repo, ctx, **args) -> Optional[str]:
    r"""Returns the repo owner for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> from .testutil import FakeContext, FakeRepo, fake_args
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> github_pull_request_repo_owner(FakeRepo(), FakeContext(descr), **fake_args())
    'bolinfest'
    """
    pull_request = get_pull_request_url_for_rev(repo, ctx, **args)
    return pull_request.owner if pull_request else None


def github_pull_request_repo_name(repo, ctx, **args) -> Optional[str]:
    r"""Returns the repo name for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> from .testutil import FakeContext, FakeRepo, fake_args
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> ctx = FakeContext(descr)
    >>> github_pull_request_repo_name(FakeRepo(), FakeContext(descr), **fake_args())
    'ghstack-testing'
    """
    pull_request = get_pull_request_url_for_rev(repo, ctx, **args)
    return pull_request.name if pull_request else None


def github_pull_request_number(repo, ctx, **args) -> Optional[int]:
    r"""Returns the number for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> from .testutil import FakeContext, FakeRepo, fake_args
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> ctx = FakeContext(descr)
    >>> github_pull_request_number(FakeRepo(), FakeContext(descr), **fake_args())
    71
    """
    pull_request = get_pull_request_url_for_rev(repo, ctx, **args)
    return pull_request.number if pull_request else None


# Special value to use as the second argument to Dict.get() to distinguish
# between "no mapping" and "mapping with a value of None".
_NO_ENTRY = {}

_GITHUB_OAUTH_TOKEN_CACHE_KEY = "github_token"
_GITHUB_PULL_REQUEST_URL_REVCACHE_KEY = "github_pr_url"
_GITHUB_PULL_REQUEST_DATA_REVCACHE_KEY = "github_pr_data"
_GITHUB_PULL_REQUEST_STORE_KEY = "github_pr_store"


def get_pull_request_url_for_rev(repo, ctx, **args) -> Optional[PullRequestId]:
    revcache = args["revcache"]
    pull_request_url = revcache.get(_GITHUB_PULL_REQUEST_URL_REVCACHE_KEY, _NO_ENTRY)
    if pull_request_url is not _NO_ENTRY:
        return pull_request_url

    store = get_pull_request_store(repo, args["cache"])
    pull_request_url = get_pull_request_for_context(store, ctx)

    revcache[_GITHUB_PULL_REQUEST_URL_REVCACHE_KEY] = (
        pull_request_url if pull_request_url is not None else _NO_ENTRY
    )
    return pull_request_url


def get_pull_request_data_for_rev(repo, ctx, **args) -> GraphQLPullRequest:
    revcache = args["revcache"]
    pull_request_data = revcache.get(_GITHUB_PULL_REQUEST_DATA_REVCACHE_KEY, _NO_ENTRY)

    # If there is a cache miss, we need both (1) an OAuth token and (2) a pull
    # request URL in the commit message to fetch pull request data.
    if pull_request_data is _NO_ENTRY:
        pull_request = get_pull_request_url_for_rev(repo, ctx, **args)
        if pull_request:
            pull_request_data = get_pull_request_data(repo, pull_request)
    return pull_request_data


def get_pull_request_store(repo, cache) -> PullRequestStore:
    store = cache.get(_GITHUB_PULL_REQUEST_STORE_KEY)
    if store:
        return store

    store = PullRequestStore(repo)
    cache[_GITHUB_PULL_REQUEST_STORE_KEY] = store
    return store
