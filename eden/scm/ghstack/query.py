# GraphQL queries for code and mocking tests to use.

GRAPHQL_GET_REPOSITORY = """
query ($owner: String!, $name: String!) {
    repository(name: $name, owner: $owner) {
        id
        isFork
        defaultBranchRef {
            name
        }
    }
}
"""

GRAPHQL_PR_TO_REF = """
query ($owner: String!, $name: String!, $number: Int!) {
    repository(name: $name, owner: $owner) {
        pullRequest(number: $number) {
            headRefName
        }
    }
}
"""

GRAPHQL_REF_TO_COMMIT_AND_TREE = """
query ($repo_id: ID!, $ref: String!) {
  node(id: $repo_id) {
    ... on Repository {
      ref(qualifiedName: $ref) {
        target {
          oid
          ... on Commit {
            tree {
              oid
            }
          }
        }
      }
    }
  }
}
"""
