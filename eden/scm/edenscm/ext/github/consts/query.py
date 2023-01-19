# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

### GraphQL queries

GRAPHQL_GET_REPOSITORY = """
query ($owner: String!, $name: String!) {
  repository(name: $name, owner: $owner) {
    id
    owner {
      id
      login
    }
    name
    isFork
    defaultBranchRef {
      name
    }
    parent {
      id
      owner {
        id
        login
      }
      name
      isFork
      defaultBranchRef {
        name
      }
    }
  }
}
"""

GRAPHQL_GET_PULL_REQUEST = """
query ($owner: String!, $name: String!, $number: Int!) {
  repository(name: $name, owner: $owner) {
    pullRequest(number: $number) {
      id
      url
      state
      baseRefOid
      baseRefName
      headRefOid
      headRefName
      body
    }
  }
}
"""

GRAPHQL_GET_MAX_PR_ISSUE_NUMBER = """
query ($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    issues(orderBy: {field: CREATED_AT, direction: ASC}, last: 1) {
      nodes {
        number
      }
    }
    pullRequests(orderBy: {field: CREATED_AT, direction: ASC}, last: 1) {
      nodes {
        number
      }
    }
  }
}
"""

GRAPHQL_UPDATE_PULL_REQUEST = """
mutation ($pullRequestId: ID!, $title: String!, $body: String!, $base: String!) {
  updatePullRequest(
    input: {pullRequestId: $pullRequestId, title: $title, body: $body, baseRefName: $base}
  ) {
    pullRequest {
      id
    }
  }
}
"""

GRAPHQL_CREATE_BRANCH = """
mutation ($repositoryId: ID!, $name: String!, $oid: GitObjectID!) {
  createRef(input: {repositoryId: $repositoryId, name: $name, oid: $oid}) {
    ref {
      id
    }
  }
}
"""

GRAPHQL_MERGE_BRANCH = """
mutation ($repositoryId: ID!, $base: String!, $head: String!) {
  mergeBranch(input: {repositoryId: $repositoryId, base: $base, head: $head}) {
    mergeCommit {
      oid
    }
  }
}
"""

GRAPHQL_GET_LOGIN = """
query {
  viewer {
    login
  }
}
"""
