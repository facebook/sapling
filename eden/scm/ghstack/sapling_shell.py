import json
import logging
import os
from typing import Any, Dict, Optional, List

from edenscm import error, gpg
from edenscm.i18n import _
from edenscm.node import hex, nullid
import ghstack
import ghstack.config
from ghstack.ghs_types import GitCommitHash
from ghstack.shell import _SHELL_RET

WILDCARD_ARG = {}

class SaplingShell(ghstack.shell.Shell):
    def __init__(self,
                 *,
                 conf: ghstack.config.Config,
                 ui = None,  # Sapling ui object.
                 repo = None,  # Sapling repo object.
                 git_dir: str,
                 user_name: str,
                 user_email: str,
                 quiet: bool = False,
                 cwd: Optional[str] = None,
                 testing: bool = False,
                 sapling_cli: str = "sl"):
        """Creates a new Shell that runs Git commands in terms of Sapling
        commands, as appropriate.

        In order for Sapling to work in the absence of a .gitconfig file,
        Git commands, such as `git commit-tree`, that expect to read `user.name`
        and `user.email` from .gitconfig will be invoked by specifying the
        config values on the command line (with -c) using the `user_name` and
        `user_email` values passed to this constructor.
        """
        super().__init__(quiet=quiet, cwd=cwd, testing=testing)
        self.conf = conf
        self.ui = ui
        self.repo = repo
        self.git_dir = git_dir
        self.user_name = user_name
        self.user_email = user_email
        self.sapling_cli = sapling_cli
        logging.debug(f"--git-dir set to: {self.git_dir}")

    def is_git(self) -> bool:
        """Whether this shell corresponds to a Git working copy."""
        return False

    def is_sapling(self) -> bool:
        """Whether this shell corresponds to a Sapling working copy."""
        return True

    def git(self, *_args: str, **kwargs: Any  # noqa: F811
            ) -> _SHELL_RET:
        args = list(_args)
        remote_name = self.conf.remote_name
        if match_args(["remote", "get-url", remote_name], args):
            return self._get_origin()
        elif match_args(["checkout"], args):
            args[0] = "goto"
        elif match_args(["fetch", "--prune"], args):
            raise ValueError(f"unexpected use of `git fetch` in SaplingShell: {' '.join(args)}")
        elif match_args(["merge-base", WILDCARD_ARG, "HEAD"], args):
            # remote is probably "origin/main", which we need to convert to
            # "main" to use with the `log` subcommand.
            remote = args[1]
            index = remote.rfind('/')
            if index != -1:
                remote = remote[(index+1):]
            return self._run_sapling_command(["log", "-T", "{node}", "-r", f"ancestor(., {remote})"])
        elif match_args(["push", remote_name], args):
            if len(args) == 2:
                raise ValueError(f"expected more args: {args}")
            args[1] = self._get_origin()
        elif match_args(["reset"], args):
            raise ValueError(f"unexpected use of `git reset` in SaplingShell: {' '.join(args)}")

        git_args = self._rewrite_args(args)
        full_args = ["--git-dir", self.git_dir] + git_args
        return super().git(*full_args, **kwargs)

    def _rewrite_args(self, _args: List[str]) -> List[str]:
        args = _args[:]

        # When running queries against a bare repo via `git --git-dir`, Git will
        # not be able to resolve arguments like HEAD, so we must resolve those
        # to a full hash before running Git.
        if 'HEAD' in args:
            # Approximate `sl whereami`.
            p1 = self.repo.dirstate.p1()
            if p1 == nullid:
                raise error.Abort(_("could not find a current commit hash"))

            for index, arg in enumerate(args):
                if arg == 'HEAD':
                    args[index] = hex(p1)

        return args

    def _get_origin(self):
        # This should be good enough, right???
        return self._run_sapling_command(["config", "paths.default"])

    def run_sapling_command(self, *args: str) -> str:
        return self._run_sapling_command(list(args))

    def _run_sapling_command(self, args: List[str]) -> str:
        env = dict(os.environ)
        env["SL_AUTOMATION"] = "true"
        full_args = [self.sapling_cli] + args
        stdout = self.sh(*full_args, env=env)
        assert isinstance(stdout, str)
        # pyre-ignore[7]
        return self._maybe_rstrip(stdout)

    def rewrite_commit_message(self, rev: str, commit_msg: str) -> Dict[str, str]:
        stdout = self.run_sapling_command(
            "metaedit", "-q", "-T", "{nodechanges|json}", "-r", rev, "-m", commit_msg)
        # Note that updates will look something like:
        #
        # {
        #   "3ee7d318bc9374566457061e1413740f7db070d6": [
        #     "d4277e2ad161ab5405323b36a67686e7404d7e97"
        #   ],
        #   "9f6bf8c9a07ac90fb550b41da4331d6cf0ea2699": [
        #     "2f5f7a68ca20c8eb9212fc9ac5dd1cb13181eb07"
        #   ],
        #   "d00ac3f69366d192413772b3815fa204280a78b5": [
        #     "9c10a04ca9e6752f71519b59647f754b0f117a14"
        #   ]
        # }
        #
        # Where each key is an original hash and the values are the updated
        # hashes. In our usage, each value should have one item in the list.
        mappings = json.loads(stdout)
        return {k: v[0] for k, v in mappings.items()}

    def git_commit_tree(self, *args, **kwargs: Any  # noqa: F811
            ) -> GitCommitHash:
        """Run `git commit-tree`, adding GPG flags, if appropriate.
        """
        config_flags = ['-c', f'user.name={self.user_name}', '-c', f'user.email={self.user_email}']
        keyid = gpg.get_gpg_keyid(self.ui)
        gpg_args = [f'-S{keyid}'] if keyid else []
        full_args = config_flags + ["commit-tree"] + gpg_args + list(args)
        stdout = self.git(*full_args, **kwargs)
        if isinstance(stdout, str):
            return GitCommitHash(stdout)
        else:
            raise ValueError(f"Unexpected `{' '.join(full_args)}` returned {stdout} using {kwargs}")


def match_args(pattern, args: List[str]) -> bool:
    if len(pattern) > len(args):
        return False

    for pattern_arg, arg in zip(pattern, args):
        if pattern_arg is WILDCARD_ARG:
            continue
        elif isinstance(pattern_arg, str):
            if pattern_arg != arg:
                return False
        else:
            raise ValueError(f"Unknown pattern type: {type(pattern_arg)}: {pattern_arg}")

    return True
