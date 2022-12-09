import logging
import re

import ghstack.github
import ghstack.github_utils
import ghstack.sapling_shell
import ghstack.shell


def main(pull_request: str,
         github: ghstack.github.GitHubEndpoint,
         sh: ghstack.shell.Shell,
         remote_name: str,
         ) -> None:

    params = ghstack.github_utils.parse_pull_request(pull_request)
    pr_result = github.graphql_sync("""
        query ($owner: String!, $name: String!, $number: Int!) {
            repository(name: $name, owner: $owner) {
                id
                pullRequest(number: $number) {
                    headRefName
                }
            }
        }
    """, **params)
    repository = pr_result["data"]["repository"]
    head_ref = repository["pullRequest"]["headRefName"]
    orig_ref = re.sub(r'/head$', '/orig', head_ref)
    if orig_ref == head_ref:
        logging.warning("The ref {} doesn't look like a ghstack reference".format(head_ref))

    # TODO: Handle remotes correctly too (so this subsumes hub)

    if isinstance(sh, ghstack.sapling_shell.SaplingShell):
        repo_id = repository["id"]
        oid = ghstack.github_utils.get_commit_and_tree_for_ref(
            github=github,
            repo_id=repo_id,
            ref=orig_ref,
        )['commit']
        sh.run_sapling_command("goto", oid)
    else:
        sh.git("fetch", "--prune", remote_name)
        sh.git("checkout", remote_name + "/" + orig_ref)
