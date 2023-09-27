# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import os.path
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import Dict, List, Optional, Tuple

import bindings

from .. import error
from ..i18n import _

from . import util
from .cmdtable import command

DEFAULT_PORT = 3011


@command(
    "web|isl",
    [
        (
            "p",
            "port",
            DEFAULT_PORT,
            _("port for Sapling Web"),
        ),
        ("", "json", False, _("output machine-readable JSON")),
        ("", "open", True, _("open Sapling Web in a local browser")),
        ("f", "foreground", False, _("keep the server process in the foreground")),
        (
            "",
            "kill",
            False,
            _("kill any running server process, but do not start a new server"),
        ),
        (
            "",
            "force",
            False,
            _("kill any running server process, then start a new server"),
        ),
        (
            "",
            "platform",
            "",
            _(
                "which environment ISL is being embedded in, used to support IDE integrations (ADVANCED)"
            ),
        ),
    ],
)
def isl_cmd(ui, repo, *args, **opts):
    """launch Sapling Web GUI on localhost

    Sapling Web is a collection of web-based tools including Interactive Smartlog,
    which is a GUI that facilitates source control operations such as creating,
    reordering, or rebasing commits.
    Running this command launches a web server that makes Sapling Web and
    Interactive Smartlog available in a local web browser.

    Examples:

    Launch Sapling Web locally on port 8081::

        $ @prog@ web --port 8081
        Listening on http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling
        Server logs will be written to /dev/shm/tmp/isl-server-logrkrmxp/isl-server.log

    Using the ``--json`` option to get the current status of Sapling Web::

        $ @prog@ web --port 8081 --json | jq
        {
            "url": "http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling",
            "port": 8081,
            "token": "bbe168b7b4af1614dd5b9ddc48e7d30e",
            "pid": 1521158,
            "wasServerReused": true,
            "logFileLocation": "/dev/shm/tmp/isl-server-logrkrmxp/isl-server.log",
            "cwd": "/home/alice/sapling"
        }

    Using the ``--kill`` option to shut down the server::

        $ @prog@ web --port 8081 --kill
        killed ISL server process 1521158
    """
    port = opts.get("port")
    open_isl = opts.get("open")
    json_output = opts.get("json")
    foreground = opts.get("foreground")
    kill = opts.get("kill")
    force = opts.get("force")
    platform = opts.get("platform")
    return launch_server(
        ui,
        cwd=repo.root,
        port=port,
        open_isl=open_isl,
        json_output=json_output,
        foreground=foreground,
        force=force,
        kill=kill,
        platform=platform,
    )


def launch_server(
    ui,
    *,
    cwd,
    port=DEFAULT_PORT,
    open_isl=True,
    json_output=False,
    foreground=False,
    kill=False,
    force=False,
    platform=None,
):
    isl_args, isl_cwd = get_isl_args_cwd(ui)
    args = [
        "--port",
        str(port),
        "--command",
        util.hgcmd()[0],
        "--cwd",
        cwd,
        "--sl-version",
        util.version(),
    ]
    if not open_isl:
        args.append("--no-open")
    if json_output:
        args.append("--json")
    if foreground:
        args.append("--foreground")
    if force:
        args.append("--force")
    if kill:
        args.append("--kill")
    if platform:
        args += ["--platform", str(platform)]
    full_args = isl_args + args
    ui.note_err(_("running %s\n") % (shlex.join(full_args),))
    subprocess.call(full_args, cwd=isl_cwd)


def untar(tar_path, dest_dir) -> Dict[str, str]:
    """untar to the destination directory, return the tar metadata (dict)"""
    os.makedirs(dest_dir, exist_ok=True)
    with tarfile.open(tar_path, "r", format=tarfile.PAX_FORMAT) as tar:
        # build-tar.py sets the "source_hash" but if it doesn't, use the file
        # size as an approx.
        source_hash = tar.pax_headers.get("source_hash") or str(
            os.stat(tar_path).st_size
        )
        existing_source_hash_path = os.path.join(dest_dir, ".source_hash")
        existing_source_hash = ""
        try:
            with open(existing_source_hash_path, "rb") as f:
                existing_source_hash = f.read().decode()
        except FileNotFoundError:
            pass
        # extract if changed
        if source_hash != existing_source_hash:
            # Delete the existing directory. Rename first for better
            # compatibility on Windows.
            if os.path.isdir(dest_dir):
                to_delete_dir = f"{dest_dir}.to-delete"
                shutil.rmtree(to_delete_dir, ignore_errors=True)
                os.rename(dest_dir, to_delete_dir)
                shutil.rmtree(to_delete_dir, ignore_errors=True)
                os.makedirs(dest_dir, exist_ok=True)
            if sys.version_info > (3, 11):
                tar.extractall(dest_dir, filter="data")
            else:
                tar.extractall(dest_dir)
            # write source_hash so we can skip extractall() next time
            with open(existing_source_hash_path, "wb") as f:
                f.write(source_hash.encode())
        return tar.pax_headers or {}


def resolve_path(candidates, which=shutil.which) -> Optional[str]:
    """resolve full path from candidates"""
    for path in candidates:
        if not os.path.isabs(path):
            path = which(path)
        if path and os.path.isfile(path):
            return path
    return None


def find_nodejs(ui) -> str:
    """find the path to nodejs, or raise if nothing found"""
    candidates = ui.configlist("web", "node-path") + ["node"]
    node_path = resolve_path(candidates)
    if node_path is None:
        raise error.Abort(_("cannot find nodejs to execute ISL"))
    return node_path


def get_isl_args_cwd(ui) -> Tuple[List[str], str]:
    # find "isl-dist.tar.xz"
    candidates = ui.configlist("web", "isl-dist-path") + ["isl-dist.tar.xz"]
    isl_tar_path = resolve_path(
        candidates, lambda p: os.path.join(os.path.dirname(sys.executable), p)
    )
    if isl_tar_path is None:
        raise error.Abort(_("ISL is not available with this @prog@ install"))

    # extract "isl-dist.tar.xz"
    data_dir = bindings.dirs.data_local_dir() or tempfile.gettempdir()
    dest_dir = os.path.join(data_dir, "Sapling", "ISL")
    ui.note_err(_("extracting %s to %s\n") % (isl_tar_path, dest_dir))
    try:
        tar_metadata = untar(isl_tar_path, dest_dir)
    except Exception as e:
        raise error.Abort(_("cannot extract ISL: %s") % (e,))

    # the args are: node entry_point ...
    node_path = find_nodejs(ui)
    entry_point = tar_metadata.get("entry_point") or "isl-server/dist/run-proxy.js"
    return [node_path, entry_point], dest_dir
