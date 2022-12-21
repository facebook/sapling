# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import itertools
import json
import os
from typing import Dict, List, Optional, TypeVar, Union, Any

from edenscm import error
from edenscm.result import Result, Ok, Err
from edenscm.i18n import _


JsonDict = Dict[str, Any]


async def make_request(
    params: Dict[str, Union[str, int, bool]],
    hostname: str,
    endpoint="graphql",
    method: Optional[str] = None,
) -> Result[JsonDict, str]:
    """If successful, returns a Result whose value is parsed JSON returned by
    the request.
    """
    if method:
        endpoint_args = ["-X", method.upper(), endpoint]
    else:
        endpoint_args = [endpoint]
    args = (
        ["gh", "api", "--hostname", hostname]
        + endpoint_args
        + list(itertools.chain(*[_format_param(k, v) for (k, v) in params.items()]))
    )

    # https://cli.github.com/manual/gh_help_environment documents support for
    # CLICOLOR and CLICOLOR_FORCE. Note that a user unknowingly had
    # CLICOLOR_FORCE=1 set in a zsh script somewhere and got a very confusing
    # error as reported on https://github.com/facebook/sapling/issues/146
    # because the output of gh could not be parsed via json.loads(), so we
    # explicitly disable ANSI colors in our piped output.
    env = os.environ.copy()
    env["CLICOLOR_FORCE"] = "0"
    proc = await asyncio.create_subprocess_exec(
        *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE, env=env
    )
    stdout, stderr = await proc.communicate()

    # If proc exits with a non-zero exit code, the stdout may still
    # be valid JSON, but we expect it to have an "errors" property defined.
    try:
        response = json.loads(stdout)
    except json.JSONDecodeError:
        response = None

    if proc.returncode == 0:
        assert response is not None
        assert "errors" not in response
        return Ok(response)
    elif response is not None:
        return Err(json.dumps(response, indent=1))
    elif b"gh auth login" in stderr:
        # The error message is likely referring to an authentication issue.
        raise error.Abort(_("Error calling the GitHub API:\n%s") % stderr.decode())
    else:
        return Err(
            f"exit({proc.returncode}) Failure running {' '.join(args)}\nstdout: {stdout.decode()}\nstderr: {stderr.decode()}\n"
        )


def _format_param(key: str, value: Union[str, int, bool]) -> List[str]:
    # In Python, bool is a subclass of int, so check it first.
    if isinstance(value, bool):
        opt = "-F"
        val = str(value).lower()
    elif isinstance(value, int):
        opt = "-F"
        val = value
    elif isinstance(value, str):
        opt = "-f"
        val = str(value)
    else:
        raise RuntimeError(f"unexpected param: {key}={value}")
    return [opt, f"{key}={val}"]
