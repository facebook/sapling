# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path
from typing import Dict, Optional, TextIO, Tuple

try:
    from __manifest__ import fbmake  # noqa: F401

    FBCODE = True
except ImportError:
    FBCODE = False

if FBCODE and os.environ.get("USE_MONONOKE", False):
    from mononoke.repos.repos.thrift_types import (
        RawAllowlistIdentity,
        RawBlobstoreConfig,
        RawBlobstoreFilePath,
        RawCommonConfig,
        RawDbLocal,
        RawDerivedDataConfig,
        RawDerivedDataTypesConfig,
        RawHookManagerParams,
        RawMetadataConfig,
        RawPushParams,
        RawPushrebaseParams,
        RawRedactionConfig,
        RawRepoConfig,
        RawRepoConfigs,
        RawRepoDefinition,
        RawRepoDefinitions,
        RawStorageConfig,
    )
    from thrift.python.serializer import Protocol, serialize

from .hg import hg
from .repo import Repo
from .util import new_dir


class Server:
    def _clone(self, repoid: int, url: str) -> Repo:
        repo_name = self.reponame(repoid)
        root = new_dir(label=f"Repository {repo_name}")

        hg(root).clone(
            url, root, noupdate=True, config=f"remotefilelog.reponame={repo_name}"
        )
        with open(os.path.join(root, ".hg", "hgrc"), "a+") as f:
            f.write(
                f"""
[remotefilelog]
reponame={repo_name}
"""
            )

        return Repo(root, url, repo_name)

    def cleanup(self) -> None:
        pass

    def reponame(self, repoid: int = 0) -> str:
        return f"repo{repoid}"


class LocalServer(Server):
    """An EagerRepo backed EdenApi server."""

    urls: Dict[int, str]

    def __init__(self) -> None:
        self.urls = {}

    def clone(self, repoid: int = 0) -> Repo:
        if repoid not in self.urls:
            self.urls[repoid] = f"eager://{new_dir()}"
        return self._clone(repoid, self.urls[repoid])


class MononokeServer(Server):
    edenapi_url: str
    port: str
    process: subprocess.Popen[bytes]
    repo_count: int
    url_prefix: str
    stderr_file: Optional[TextIO] = None

    def __init__(self, record_stderr_to_file: bool, repo_count: int = 5) -> None:
        temp_dir = new_dir(label="Server Dir")

        if record_stderr_to_file:
            self.stderr_file = open(  # noqa: P201
                os.path.join(temp_dir, "mononoke-log"), "w+"
            )

        # pyre-fixme[23]: Unable to unpack 3 values, 2 were expected.
        self.process, self.port = _start(temp_dir, repo_count, self.stderr_file)
        self.repo_count = repo_count

        self.url_prefix = f"mononoke://localhost:{self.port}"
        self.edenapi_url = f"https://localhost:{self.port}/edenapi"

    def clone(self, repoid: int = 0) -> Repo:
        if repoid >= self.repo_count:
            raise ValueError(
                "cannot request repo %s when there are only %s repos"
                % (repoid, self.repo_count)
            )
        return self._clone(repoid, self.url_prefix + "/" + self.reponame(repoid))

    def cleanup(self) -> None:
        self.process.kill()
        self.process.wait(timeout=5)

        if self.stderr_file is not None:
            stderr_file: TextIO = self.stderr_file
            stderr_file.flush()
            stderr_file.close()


def _start(
    temp_dir: Path, repo_count: int, stderr_file: Optional[TextIO]
) -> Tuple[subprocess.Popen[bytes], str, str]:
    executable = os.environ["HGTEST_MONONOKE_SERVER"]
    cert_dir = os.environ["HGTEST_CERTDIR"]
    bind_addr = "[::1]:0"  # Localhost
    configerator_path = str(new_dir(label="Configerator Path"))
    tunables_path = "mononoke_tunables.json"

    def tjoin(path: str) -> str:
        return os.path.join(temp_dir, path)

    # pyre-fixme[53]: Captured variable `cert_dir` is not annotated.
    def cjoin(path: str) -> str:
        return os.path.join(cert_dir, path)

    addr_file = tjoin("mononoke_server_addr.txt")
    config_path = tjoin("mononoke-config")

    _setup_configerator(configerator_path)

    repo_confs, repo_defs = _setup_repos(repo_count)

    mononoke_config = _setup_mononoke_configs(config_path, repo_defs, repo_confs)

    if stderr_file:
        stderr = stderr_file
        print(f"Recording Mononoke stderr log to {stderr_file.name}")
    else:
        stderr = subprocess.PIPE

    process = subprocess.Popen(
        [
            executable,
            "--scribe-logging-directory",
            tjoin("scribe_logs"),
            "--ca-pem",
            cjoin("root-ca.crt"),
            "--private-key",
            cjoin("localhost.key"),
            "--cert",
            cjoin("localhost.crt"),
            "--ssl-ticket-seeds",
            cjoin("server.pem.seeds"),
            "--listening-host-port",
            bind_addr,
            "--bound-address-file",
            addr_file,
            "--mononoke-config-path",
            mononoke_config,
            "--no-default-scuba-dataset",
            "--debug",
            "--skip-caching",
            "--mysql-master-only",
            "--tunables-config",
            tunables_path,
            "--local-configerator-path",
            configerator_path,
            "--log-exclude-tag",
            "futures_watchdog",
            "--with-test-megarepo-configs-client=true",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=stderr,
        close_fds=True,
    )

    port = _wait(
        process,
        addr_file,
        cjoin("client0.crt"),
        cjoin("client0.key"),
        cjoin("root-ca.crt"),
    )

    # pyre-fixme[7]: Expected `Tuple[Popen[bytes], str, str]` but got
    #  `Tuple[Popen[typing.Any], Tuple[str, str]]`.
    return process, port


def _wait(
    # pyre-fixme[24]: Generic type `subprocess.Popen` expects 1 type parameter.
    process: subprocess.Popen,
    addr_file: str,
    cert: str,
    key: str,
    ca_cert: str,
) -> Tuple[str, str]:
    start = time.time()
    while not os.path.exists(addr_file) and (time.time() - start < 60):
        time.sleep(0.5)
        state = process.poll()
        if state is not None:
            raise Exception(
                "Mononoke server exited early (%s):\n%s\n%s"
                # pyre-fixme[16]: Optional type has no attribute `read`.
                % (state, process.stdout.read(), process.stderr.read())
            )

    if os.path.exists(addr_file):
        with open(addr_file) as f:
            content = f.read()
        split_idx = content.rfind(":")
        port = content[split_idx + 1 :].strip()
    else:
        raise Exception(
            "timed out waiting for Mononoke server %s" % (time.time() - start)
        )

    import requests

    response = requests.get(
        f"https://localhost:{port}/health_check", cert=(cert, key), verify=ca_cert
    )
    response.raise_for_status()

    # pyre-fixme[7]: Expected `Tuple[str, str]` but got `str`.
    return port


def _setup_mononoke_configs(
    config_dir: str,
    repo_definitions: Dict[str, RawRepoDefinition],
    repo_configs: Dict[str, RawRepoConfig],
) -> str:
    blobstore_name = "blobstore"
    blobstore_path = os.path.join(config_dir, blobstore_name)

    metadata_storage = RawMetadataConfig(
        local=RawDbLocal(local_db_path=str(new_dir(label="Mononoke DB")))
    )
    blobstore = RawBlobstoreConfig(
        blob_sqlite=RawBlobstoreFilePath(path=blobstore_path)
    )
    storage_config = RawStorageConfig(metadata=metadata_storage, blobstore=blobstore)
    redaction_config = RawRedactionConfig(
        blobstore=blobstore_name,
        redaction_sets_location="scm/mononoke/redaction/redaction_sets",
    )
    allowlist_entry = RawAllowlistIdentity(
        identity_type="USER", identity_data="myusername0"
    )
    common_config = RawCommonConfig(
        enable_http_control_api=True,
        redaction_config=redaction_config,
        internal_identity=RawAllowlistIdentity(
            identity_type="SERVICE_IDENTITY", identity_data="internal"
        ),
        global_allowlist=[allowlist_entry],
    )
    mononoke_config: RawRepoConfigs = RawRepoConfigs(
        commit_sync={},
        common=common_config,
        repos=repo_configs,
        storage={blobstore_name: storage_config},
        acl_region_configs={},
        repo_definitions=RawRepoDefinitions(repo_definitions=repo_definitions),
    )

    config_file = os.path.join(config_dir, "config.json")
    Path(config_file).parent.mkdir(parents=True, exist_ok=True)
    with open(config_file, "w+") as f:
        f.write(serialize(mononoke_config, protocol=Protocol.JSON).decode("utf-8"))

    return config_file


def _setup_repos(
    repo_count: int,
) -> Tuple[Dict[str, RawRepoConfig], Dict[str, RawRepoDefinition]]:
    repo_defs = {}
    repo_confs = {}
    for repoid in range(repo_count):
        repo_name = f"repo{repoid}"
        repo_confs[repo_name], repo_defs[repo_name] = _setup_repo(repoid, repo_name)

    return (repo_confs, repo_defs)


def _setup_repo(repoid: int, repo_name: str) -> Tuple[RawRepoConfig, RawRepoDefinition]:
    repo_def = RawRepoDefinition(
        repo_id=repoid,
        repo_name=repo_name,
        repo_config=repo_name,
    )

    derived_data_types = RawDerivedDataTypesConfig(
        types={
            "blame",
            "changeset_info",
            "deleted_manifest",
            "fastlog",
            "filenodes",
            "fsnodes",
            "unodes",
            "hgchangesets",
            "skeleton_manifests",
        }
    )
    derived_data_config = RawDerivedDataConfig(
        available_configs={"default": derived_data_types},
        enabled_config_name="default",
    )
    hook_params = RawHookManagerParams(disable_acl_checker=True)
    pushrebase_params = RawPushrebaseParams(
        rewritedates=False, forbid_p2_root_rebases=False
    )
    push_params = RawPushParams(pure_push_allowed=True)

    repo_config = RawRepoConfig(
        derived_data_config=derived_data_config,
        storage_config="blobstore",
        hook_manager_params=hook_params,
        pushrebase=pushrebase_params,
        push=push_params,
    )

    return (repo_config, repo_def)


def _setup_configerator(cfgr_root: str) -> None:
    def write(path: str, content: str) -> None:
        path = os.path.join(cfgr_root, path)
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w+") as f:
            f.write(content)

    write(
        "scm/mononoke/ratelimiting/ratelimits",
        """
{
  "rate_limits": [],
  "load_shed_limits": [],
  "datacenter_prefix_capacity": {},
  "commits_per_author": {
    "status": 0,
    "limit": 300,
    "window": 1800
  },
  "total_file_changes": {
    "status": 0,
    "limit": 80000,
    "window": 5
  }
}
""",
    )
    write(
        "scm/mononoke/pushredirect/enable",
        """
{
  "per_repo": {}
}
""",
    )
    write(
        "scm/mononoke/repos/commitsyncmaps/all",
        """
{}
""",
    )
    write(
        "scm/mononoke/repos/commitsyncmaps/current",
        """
{}
""",
    )
    write(
        "scm/mononoke/xdb_gc/default",
        """
{
  "put_generation": 2,
  "mark_generation": 1,
  "delete_generation": 0
}
""",
    )
    write(
        "scm/mononoke/observability/observability_config",
        """
{
  "slog_config": {
    "level": 4
  },
  "scuba_config": {
    "level": 1,
    "verbose_sessions": [],
    "verbose_unixnames": [],
    "verbose_source_hostnames": []
  }
}
""",
    )
    write(
        "scm/mononoke/redaction/redaction_sets",
        """
{
  "all_redactions": []
}
""",
    )
    write(
        "mononoke_tunables.json",
        """
{}
""",
    )
