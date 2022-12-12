# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""ghstack for Sapling (EXPERIMENTAL)
"""

import logging

from edenscm import error, git, gituser, rcutil, registrar, util
from edenscm.ext.github.github_repo_util import check_github_repo
from edenscm.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

import ghstack
import ghstack.action
import ghstack.checkout
import ghstack.config
import ghstack.github_cli_endpoint
import ghstack.land
import ghstack.logs
import ghstack.sapling_shell
import ghstack.submit
import ghstack.unlink


@command(
    "ghstack",
    [],
    _("{submit,unlink,land,checkout,action} (default: submit)"),
)
def ghstack_command(ui, repo, *args, **opts) -> None:
    """submits a stack of commits to GitHub as individual pull requests using ghstack

    Uses the scheme employed by ghstack (https://github.com/ezyang/ghstack) to
    submit a stack of commits to GitHub as individual pull requests. Pull
    requests managed by ghstack are never force-pushed.

    Currently, you must configure ghstack by creating a ~/.ghstackrc file as
    explained on https://github.com/ezyang/ghstack. Ultimately, we will likely
    replace this use of the GitHub CLI to manage API requests to GitHub.

    Note that you must have *write* access to the GitHub repository in order to
    use ghstack. If you do not have write access, consider using the `pr`
    subcommand instead.
    """
    return submit_cmd(ui, repo, *args, **opts)


subcmd = ghstack_command.subcommand(
    categories=[
        (
            "Create or update pull requests",
            ["submit", "unlink", "land", "checkout", "action"],
        ),
    ]
)


@subcmd(
    "submit",
    [
        ("m", "message", "Update", _("message describing changes to updated commits")),
        (
            "u",
            "update-fields",
            False,
            _("update GitHub pull request summary from the local commit"),
        ),
        ("", "short", False, _("print only the URL of the latest opened PR to stdout")),
        (
            "",
            "force",
            False,
            _("force push the branch even if your local branch is stale"),
        ),
        (
            "",
            "skip",
            False,
            _(
                "never skip pushing commits, even if the contents didn't change"
                " (use this if you've only updated the commit message)."
            ),
        ),
        (
            "",
            "draft",
            False,
            _(
                "create the pull request in draft mode "
                "(only if it has not already been created)"
            ),
        ),
    ],
)
def submit_cmd(ui, repo, *args, **opts) -> None:
    """submit stack of commits to GitHub"""
    conf, sh, github = _create_ghstack_context(ui, repo)
    ghstack.submit.main(
        msg=opts.get("message"),
        username=conf.github_username,
        sh=sh,
        github=github,
        update_fields=opts.get("update-fields", False),
        short=opts.get("short", False),
        force=opts.get("force", False),
        no_skip=not opts.get("skip"),
        draft=opts.get("draft", False),
        github_url=conf.github_url,
        remote_name=conf.remote_name,
    )


@subcmd(
    "unlink",
    [],
    _("COMMITS..."),
)
def unlink_cmd(ui, repo, *args, **opts) -> None:
    """remove the association of a commit with a pull request"""
    conf, sh, github = _create_ghstack_context(ui, repo)
    commits = list(args)
    ghstack.unlink.main(
        commits=commits,
        github=github,
        sh=sh,
        github_url=conf.github_url,
        remote_name=conf.remote_name,
    )


@subcmd(
    "land",
    [],
    _("PR"),
)
def land_cmd(ui, repo, *args, **opts) -> None:
    """lands the stack for the specified pull request URL"""
    conf, sh, github = _create_ghstack_context(ui, repo)
    if len(args) != 1:
        raise error.Abort(_("must specify a URL for a pull request"))

    pull_request = args[0]
    ghstack.land.main(
        pull_request=pull_request,
        github=github,
        sh=sh,
        github_url=conf.github_url,
        remote_name=conf.remote_name,
    )


@subcmd(
    "checkout",
    [],
    _("PR"),
)
def checkout_cmd(ui, repo, *args, **opts) -> None:
    """goto the stack for the specified pull request URL"""
    conf, sh, github = _create_ghstack_context(ui, repo)
    if len(args) != 1:
        raise error.Abort(_("must specify a URL for a pull request"))

    pull_request = args[0]
    ghstack.checkout.main(
        pull_request=pull_request,
        github=github,
        sh=sh,
        remote_name=conf.remote_name,
    )


@subcmd(
    "action",
    [
        (
            "",
            "close",
            False,
            _("close the specified pull request"),
        ),
    ],
    _("PR"),
)
def action_cmd(ui, repo, *args, **opts) -> None:
    """goto the stack for the specified pull request URL"""
    conf, sh, github = _create_ghstack_context(ui, repo)
    if len(args) != 1:
        raise error.Abort(_("must specify a URL for a pull request"))

    pull_request = args[0]
    ghstack.action.main(
        pull_request=pull_request,
        github=github,
        close=opts.get("close"),
    )


def _create_ghstack_context(ui, repo):
    stderr_level = logging.WARN
    if ui.debugflag:
        stderr_level = logging.DEBUG
    elif ui.verbose:
        stderr_level = logging.INFO

    cli = util.hgcmd()[0]

    ghstack.logs.setup(
        stderr_level=stderr_level,
        sapling_cli=cli,
    )

    ghstack.logs.rotate()

    github_repo = check_github_repo(repo)
    github = ghstack.github_cli_endpoint.GitHubCLIEndpoint(github_repo.hostname)
    config_section = "ghstack"
    username_config_name = "github_username"
    github_username = ui.config(config_section, username_config_name)
    if not github_username:
        github_username = github.graphql_sync(
            """
query UsernameQuery {
  viewer {
    login
  }
}
    """
        )["data"]["viewer"]["login"]
        # Write ghstack.github_username back to the user's config so we don't
        # have to pay the cost of requesting it each time. To be conservative,
        # we write it to the "local" config instead of the "user" config in the
        # event that the user has both personal and GitHub Enterprise accounts
        # authenticated with gh.
        configfile = repo.localvfs.join(ui.identity.configrepofile())
        rcutil.editconfig(
            configfile, config_section, username_config_name, github_username
        )

    github_url = ui.config(config_section, "github_url", github_repo.hostname)
    remote_name = ui.config(config_section, "remote_name", "origin")
    conf = ghstack.config.Config(
        proxy=None,
        github_oauth=None,
        github_username=github_username,
        circle_token=None,
        github_url=github_url,
        remote_name=remote_name,
        # As noted in config.py, these parameters are not used by ghstack,
        # but other tools that use ghstack as a library, so we hardcode them to
        # the empty string to satisfy the typechecker.
        fbsource_path="",
        github_path="",
        default_project_dir="",
    )
    git_dir = git.readgitdir(repo)
    user_name, user_email = gituser.get_identity_or_raise(ui)
    sh = ghstack.sapling_shell.SaplingShell(
        conf=conf,
        ui=ui,
        git_dir=git_dir,
        user_name=user_name,
        user_email=user_email,
        sapling_cli=cli,
    )
    return conf, sh, github
