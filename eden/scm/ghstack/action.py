import logging
from typing import Optional

import ghstack.github
import ghstack.github_utils
import ghstack.shell


def main(pull_request: str,
         github: ghstack.github.GitHubEndpoint,
         close: bool = False,
         ) -> None:

    params = ghstack.github_utils.parse_pull_request(pull_request)
    pr_result = github.graphql_sync("""
        query ($owner: String!, $name: String!, $number: Int!) {
            repository(name: $name, owner: $owner) {
                pullRequest(number: $number) {
                    id
                }
            }
        }
    """, **params)
    pr_id = pr_result["data"]["repository"]["pullRequest"]["id"]

    if close:
        logging.info("Closing {owner}/{name}#{number}".format(**params))
        github.graphql_sync("""
            mutation ($input: ClosePullRequestInput!) {
                closePullRequest(input: $input) {
                    clientMutationId
                }
            }
        """, input={"pullRequestId": pr_id, "clientMutationId": "A"})
