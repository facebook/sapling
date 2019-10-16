#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import datetime
import os
import subprocess
import tempfile
import time
import typing
from typing import Dict, List, Optional

from . import repobase
from .error import CommandError
from .find_executables import FindExe


class GitError(CommandError):
    pass


class GitRepository(repobase.Repository):
    def __init__(self, path: str) -> None:
        super().__init__(path)
        self.git_bin = FindExe.GIT

    def git(
        self, *args: str, encoding: str = "utf-8", env: Optional[Dict[str, str]] = None
    ) -> str:
        """
        Invoke a git command inside the repository.

        All non-keyword arguments are treated as arguments to git.

        A keyword argument of "env" can be used to specify a dictionary of
        additional environment variables to be passed to git.  (These will be
        added to the current environment.)

        "env" is currently the only valid keyword argument.

        Example usage:

          repo.git('commit', '-m', 'my new commit',
                   env={'GIT_AUTHOR_NAME': 'John Doe'})
        """
        cmd = [self.git_bin] + list(args)

        git_env = None
        if env is not None:
            git_env = os.environ.copy()
            git_env.update(env)

        try:
            completed_process = subprocess.run(
                cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=True,
                cwd=self.path,
                env=git_env,
            )
        except subprocess.CalledProcessError as ex:
            raise GitError(ex) from ex
        # pyre-fixme[22]: The cast is redundant.
        return typing.cast(str, completed_process.stdout.decode(encoding))

    def init(self) -> None:
        self.git("init")

    def get_type(self) -> str:
        return "git"

    def get_head_hash(self) -> str:
        return self.git("rev-parse", "HEAD").rstrip()

    def get_canonical_root(self) -> str:
        return os.path.join(self.path, ".git")

    def add_files(self, paths: List[str]) -> None:
        self.git("add", *paths)

    def remove_files(self, paths: List[str], force: bool = False) -> None:
        if force:
            self.git("rm", "--force", *paths)
        else:
            self.git("rm", *paths)

    def commit(
        self,
        message: str,
        author_name: Optional[str] = None,
        author_email: Optional[str] = None,
        date: Optional[datetime.datetime] = None,
        amend: bool = False,
        committer_name: Optional[str] = None,
        committer_email: Optional[str] = None,
        committer_date: Optional[datetime.datetime] = None,
    ) -> str:
        if author_name is None:
            author_name = self.author_name
        if author_email is None:
            author_email = self.author_email
        if date is None:
            date = self.get_commit_time()
            date_str = time.strftime("%Y-%m-%dT%H:%M:%S%z", date.utctimetuple())
        if committer_name is None:
            committer_name = author_name
        if committer_email is None:
            committer_email = author_email
        if committer_date is None:
            committer_date = date
            committer_date_str = time.strftime(
                "%Y-%m-%dT%H:%M:%S%z", committer_date.utctimetuple()
            )

        # Specify all arguments to `git commit` to ensure the resulting hashes
        # are the same every time this test is run.
        git_commit_env = {
            "GIT_AUTHOR_NAME": author_name,
            "GIT_AUTHOR_EMAIL": author_email,
            # pyre-fixme[18]: Global name `date_str` is undefined.
            "GIT_AUTHOR_DATE": date_str,
            "GIT_COMMITTER_NAME": committer_name,
            "GIT_COMMITTER_EMAIL": committer_email,
            # pyre-fixme[18]: Global name `committer_date_str` is undefined.
            "GIT_COMMITTER_DATE": committer_date_str,
        }

        with tempfile.NamedTemporaryFile(
            prefix="eden_commit_msg.", mode="w", encoding="utf-8"
        ) as msgf:
            msgf.write(message)
            msgf.flush()

            args = ["commit", "-F", msgf.name]
            if amend:
                args.append("--amend")
            self.git(*args, env=git_commit_env)

        # Get the commit ID and return it
        return self.git("rev-parse", "HEAD").strip()
