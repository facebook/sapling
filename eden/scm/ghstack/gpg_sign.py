"""This module determines if the commits need to be signed.
We need to do this manually, because ghstack uses commit-tree instead of commit.
commit-tree command doesn't pick up commit.gpgsign git config

The porcelain git behavior w.r.t. signing is

when both `commit.gpgsign` and `user.signingkey` are set, the commit is signed
when only `commit.gpgsign` is true, git errors out

This module will retain this behavior:
We will attempt to sign as long as `commit.gpgsign` is true.
If not key is configure, error will occur
"""
import subprocess
from typing import Tuple, Union

import ghstack.shell

_should_sign = None


def gpg_args_if_necessary(
    shell: ghstack.shell.Shell = ghstack.shell.Shell()
) -> Union[Tuple[str], Tuple[()]]:
    global _should_sign
    # cache the config result
    if _should_sign is None:
        # If the config is not set, we get exit 1
        try:
            # Why the complicated compare
            # https://git-scm.com/docs/git-config#Documentation/git-config.txt-boolean
            _should_sign = shell.git("config", "--get", "commit.gpgsign") in ("yes", "on", "true", "1")
        except (subprocess.CalledProcessError, RuntimeError):
            # Note shell.git() raises RuntimeError for a non-zero exit code.
            _should_sign = False

    return ("-S",) if _should_sign else ()
