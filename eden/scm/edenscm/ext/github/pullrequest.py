# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from typing import Dict, Union


@dataclass
class PullRequestId:
    """A structured representation of the fields used to identify a pull
    request: owner, name, number.
    """

    # In GitHub, a "RepositoryOwner" is either an "Organization" or a "User":
    # https://docs.github.com/en/graphql/reference/interfaces#repositoryowner
    owner: str
    # Name of the GitHub repo within the organization.
    name: str
    # Public-facing integer ID for the pull request.
    number: int

    def as_url(self, domain=None) -> str:
        domain = domain or "github.com"
        return f"https://{domain}/{self.owner}/{self.name}/pull/{self.number}"

    def as_dict(self) -> Dict[str, Union[str, int]]:
        """Returns this PullRequestId as a Dict that can be serialized as JSON."""
        return {
            "owner": self.owner,
            "name": self.name,
            "number": self.number,
        }


class GraphQLPullRequest:
    """This object represents the information we have about a pull request that
    we got via GitHub's GraphQL API.

    When crossing the Python/Rust boundary to make a GraphQL call, we get the
    result back as a Dict on the Python side, which should be passed to the
    constructor of this class. We use the class to wrap the Dict so that we can
    provide typesafe methods on top of it.

    See pull_request_query.rs for details on the stucture of the Dict.
    """

    def __init__(self, graphql_data_as_dict):
        self.graphql_data = graphql_data_as_dict

    def __getitem__(self, key):
        return self.graphql_data[key]

    def node_id(self) -> str:
        return self.graphql_data["id"]

    def number(self) -> int:
        return self.graphql_data["number"]

    def url(self) -> str:
        return self.graphql_data["url"]

    def get_head_oid(self) -> str:
        return self.graphql_data["head"]["ref_oid"]

    def get_head_branch_name(self) -> str:
        return self.graphql_data["head"]["ref_name"]
