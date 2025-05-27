# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import re
import subprocess
import time
from typing import BinaryIO, List, Union
from urllib.parse import quote, unquote

from ..sh.bufio import BufIO
from ..sh.interp import interpcode
from ..sh.types import Env, ShellFS
from ..t.runtime import TestTmp
from .hg import hg as hgcmd


def testsetup(t: TestTmp):
    pipedown_envvars(t)
    setupfuncs(t)
    setupmatching(t)
    t.sheval("setup_environment_variables")
    # Note: we need to make sure that the Mononoke extension starts after the EdenFS one
    if t.getenv("HGTEST_USE_EDEN") == "1":
        # See comment in tests/eden.py for why we do this
        interpcode("eden start", t.shenv)
        time.sleep(3)


def setupmatching(t: TestTmp):
    localip = t.getenv("LOCALIP", "127.0.0.1")
    t.substitutions += [
        # [ipv6]:port
        (
            r"([^0-9:])\[%s\]:[0-9]+" % re.escape(localip),
            r"\1$LOCALIP:$LOCAL_PORT",
        ),
        # [ipv6]
        (
            r"([^0-9:])\[%s\]" % re.escape(localip),
            r"\1$LOCALIP",
        ),
        # ipv4:port
        (
            r"([^0-9])%s:[0-9]+" % re.escape(localip),
            r"\1$LOCALIP:$LOCAL_PORT",
        ),
        # [ipv4]
        (r"([^0-9])%s" % re.escape(localip), r"\1$LOCALIP"),
        # localhost:port
        (
            r"([^0-9])localhost:[0-9]+",
            r"\1localhost:$LOCAL_PORT",
        ),
    ]


def pipedown_envvars(t: TestTmp):
    # These env vars are set up at the TARGET level, and need to be piped down
    # to be used later on various places in Mononoke .t tests
    monoenvs = [
        "USE_MONONOKE",
        "FB_TEST_FIXTURES",
        "TEST_FIXTURES",
        "JUST_KNOBS_DEFAULTS",
        "HGTEST_CERTDIR",
        "MONONOKE_SERVER",
        "GET_FREE_SOCKET",
        "URLENCODE",
        "HGTEST_DUMMYSSH",
    ]
    for m in monoenvs:
        if em := os.environ.get(m):
            t.setenv(m, em)


def setupfuncs(t: TestTmp):
    t.command(mononoke)
    t.command(wait_for_mononoke)
    t.command(wait_for_server)
    t.command(mononoke_health)
    t.command(mononoke_address)
    t.command(mononoke_host)
    t.command(sslcurl)
    t.command(sslcurlas)
    t.command(setup_common_config)
    t.command(setup_configerator_configs)
    t.command(setup_common_hg_configs)
    t.command(setup_mononoke_config)
    t.command(setup_acls)
    t.command(setup_mononoke_repo_config)
    t.command(write_infinitepush_config)
    t.command(urlencode)
    t.command(setup_mononoke_storage_config)
    t.command(ephemeral_db_config)
    t.command(db_config)
    t.command(blobstore_db_config)
    t.command(setup_environment_variables)


def mononoke(args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env) -> int:
    scribe_logs_dir = env.getenv("SCRIBE_LOGS_DIR")
    if not fs.exists(scribe_logs_dir):
        fs.mkdir(scribe_logs_dir)

    setup_configerator_configs(fs, env)

    localip = env.getenv("LOCALIP")
    if ":" in localip:
        # IPv6, surround in brackets
        bind_addr = f"[{localip}]:0"
    else:
        bind_addr = f"{localip}:0"

    # Stop any confusion from previous runs
    mononoke_server_addr_file = env.getenv("MONONOKE_SERVER_ADDR_FILE")
    fs.rm(mononoke_server_addr_file)

    # Set environment variables
    env.setenv("PYTHONWARNINGS", "ignore:::requests,ignore::SyntaxWarning")
    env.setenv("GLOG_minloglevel", "5")

    # Prepare command and arguments
    mononoke_server = env.getenv("MONONOKE_SERVER")
    test_tmp = env.getenv("TESTTMP")
    test_certdir = env.getenv("TEST_CERTDIR")
    cache_args = env.getenv("CACHE_ARGS_U", "").split(" ")
    common_args = env.getenv("COMMON_ARGS_U", "").split(" ")
    mononoke_command = [
        mononoke_server,
        *args,
        "--scribe-logging-directory",
        f"{test_tmp}/scribe_logs",
        "--tls-ca",
        f"{test_certdir}/root-ca.crt",
        "--tls-private-key",
        f"{test_certdir}/localhost.key",
        "--tls-certificate",
        f"{test_certdir}/localhost.crt",
        "--tls-ticket-seeds",
        f"{test_certdir}/server.pem.seeds",
        "--land-service-client-cert",
        f"{test_certdir}/proxy.crt",
        "--land-service-client-private-key",
        f"{test_certdir}/proxy.key",
        "--debug",
        "--listening-host-port",
        bind_addr,
        "--bound-address-file",
        mononoke_server_addr_file,
        "--mononoke-config-path",
        f"{test_tmp}/mononoke-config",
        "--no-default-scuba-dataset",
        *cache_args,
        *common_args,
    ]

    if not env.getenv("ENABLE_BOOKMARK_CACHE"):
        mononoke_command.append("--disable-bookmark-cache-warming")

    # Execute the command in the background
    with open(f"{test_tmp}/mononoke.out", "w") as outfile:
        try:
            mononoke_proc = subprocess.Popen(
                mononoke_command,
                stdout=outfile,
                stderr=outfile,
                env=env.getexportedenv(),
            )
        except:
            stderr.write(
                f"Error when running mononoke with command {mononoke_command} and stdout file {test_tmp}/mononoke.out\n".encode()
            )
    env.setenv("MONONOKE_PID", str(mononoke_proc.pid))
    daemon_pids = env.getenv("DAEMON_PIDS")
    with fs.open(daemon_pids, "a") as f:
        f.write(f"{mononoke_proc.pid}\n".encode())

    return 0


def wait_for_mononoke(args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env) -> int:
    test_tmp = env.getenv("TESTTMP")
    mononoke_server_addr_file = env.getenv("MONONOKE_SERVER_ADDR_FILE")
    mononoke_default_start_timeout = env.getenv("MONONOKE_DEFAULT_START_TIMEOUT")
    mononoke_start_timeout = env.getenv(
        "MONONOKE_START_TIMEOUT", mononoke_default_start_timeout
    )
    if not env.getenv("MONONOKE_SOCKET"):
        env.setenv("MONONOKE_SOCKET", "")

    # Wait for the Mononoke server to be ready
    rv = wait_for_server(
        [
            "Mononoke",
            "MONONOKE_SOCKET",
            f"{test_tmp}/mononoke.out",
            mononoke_start_timeout,
            mononoke_server_addr_file,
            "mononoke_health",
        ],
        stderr,
        fs,
        env,
    )
    if rv != 0:
        return rv

    # Retrieve the Mononoke address
    mononoke_address_v = mononoke_address(env)
    mononoke_socket = env.getenv("MONONOKE_SOCKET")
    # Update the HGRCPATH configuration
    hg_rc_path = env.getenv("HGRCPATH")
    with fs.open(hg_rc_path, "a") as f:
        f.write(
            f"""
[schemes]
mono=mononoke://{mononoke_address_v}/{{1}}
test=mononoke://{mononoke_address_v}/{{1}}
[edenapi]
url=https://localhost:{mononoke_socket}/edenapi/
""".encode()
        )
    return 0


def wait_for_server(args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env) -> int:
    if len(args) < 5:
        stderr.write(b"Error: Not enough arguments provided to wait_for_server\n")
        return 1

    service_description = args[0]
    port_env_var = args[1]
    log_file = args[2]
    timeout_secs = int(args[3])
    bound_addr_file = args[4]
    health_check_command = " ".join(args[5:])

    start_time = time.time()
    found_port = None

    while (time.time() - start_time) < timeout_secs:
        if not found_port and fs.exists(bound_addr_file):
            with fs.open(bound_addr_file, "r") as f:
                content = f.read().decode()
                found_port = content.split(":")[-1].strip()
                env.setenv(port_env_var, found_port)

        if found_port:
            # Execute the health check command
            result = interpcode(health_check_command, env).exitcode
            if result == 0:
                return 0

        time.sleep(1)

    elapsed_time = int(time.time() - start_time)
    stderr.write(
        f"{service_description} did not start in {timeout_secs} seconds, took {elapsed_time}\n".encode()
    )
    if found_port:
        stderr.write(
            f"Running check: {' '.join(health_check_command)} >/dev/null\n".encode()
        )
        result = interpcode(health_check_command, env).exitcode
        stderr.write(f"exited with {result}\n".encode())
    else:
        stderr.write(f"Port was never written to {bound_addr_file}\n".encode())

    stderr.write(b"\nLog of {service_description}:\n")
    with fs.open(log_file, "r") as f:
        stderr.write(f.read())

    return 1


def mononoke_health(stderr: BinaryIO, env: Env) -> Union[str, int]:
    mononoke_socket = env.getenv("MONONOKE_SOCKET")
    if not mononoke_socket:
        stderr.write(b"Error: MONONOKE_SOCKET environment variable is not set\n")
        return 1

    url = f"https://localhost:{mononoke_socket}/health_check"
    return sslcurl(["-q", url], stderr, env)


def mononoke_address(env: Env) -> str:
    localip = env.getenv("LOCALIP")
    monosock = env.getenv("MONONOKE_SOCKET")
    if ":" in localip:
        return f"[{localip}]:{monosock}"
    else:
        return f"{localip}:{monosock}"


def mononoke_host(env: Env) -> str:
    localip = env.getenv("LOCALIP")
    if ":" in localip:
        return f"[{localip}]"
    else:
        return f"{localip}"


def sslcurl(args: List[str], stderr: BinaryIO, env: Env) -> Union[str, int]:
    return sslcurlas(["proxy"] + [*args], stderr, env)


def sslcurlas(args: List[str], stderr: BinaryIO, env: Env) -> Union[str, int]:
    name = args[0]
    test_certdir = env.getenv("TEST_CERTDIR")
    if not test_certdir:
        stderr.write(b"Error: TEST_CERTDIR environment variable is not set\n")
        return 1

    cert_path = f"{test_certdir}/{name}.crt"
    key_path = f"{test_certdir}/{name}.key"
    cacert_path = f"{test_certdir}/root-ca.crt"
    headers = 'x-client-info: {"request_info": {"entry_point": "CurlTest", "correlator": "test"}}'

    # TODO: Ideally we shouldn't specify the full path here, but in some cases
    # $PATH might be already lost, so just using "curl" doesn't cut it.
    curl_command = [
        "/usr/bin/curl",
        "--noproxy",
        "localhost",
        "-s",
        "-H",
        headers,
        "--cert",
        cert_path,
        "--cacert",
        cacert_path,
        "--key",
        key_path,
    ] + [*args[1:]]

    result = subprocess.run(curl_command, capture_output=True)

    stderr.write(result.stderr)

    if result.returncode != 0:
        return result.returncode

    return result.stdout.decode()


def setup_common_config(
    args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env
) -> int:
    if (rv := setup_mononoke_config(args, stderr, fs, env)) != 0:
        return rv
    if (rv := setup_common_hg_configs(fs, env)) != 0:
        return rv
    return setup_configerator_configs(fs, env)


def setup_configerator_configs(fs: ShellFS, env: Env) -> int:
    local_configerator_path = env.getenv("LOCAL_CONFIGERATOR_PATH")
    test_fixtures = env.getenv("TEST_FIXTURES")
    skip_cross_repo_config = env.getenv("SKIP_CROSS_REPO_CONFIG")

    # Setup Rate Limit Config
    rate_limit_conf = f"{local_configerator_path}/scm/mononoke/ratelimiting"
    env.setenv("RATE_LIMIT_CONF", rate_limit_conf)
    fs.mkdir(rate_limit_conf)
    if not fs.exists(f"{rate_limit_conf}/ratelimits"):
        with fs.open(f"{rate_limit_conf}/ratelimits", "w") as f:
            f.write(
                b"""
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
"""
            )

    # Setup Commit Sync Config
    commit_sync_conf = f"{local_configerator_path}/scm/mononoke/repos/commitsyncmaps"
    env.setenv("COMMIT_SYNC_CONF", commit_sync_conf)
    fs.mkdir(commit_sync_conf)
    if skip_cross_repo_config:
        with fs.open(f"{commit_sync_conf}/all", "w") as f:
            f.write(b"{}")
        with fs.open(f"{commit_sync_conf}/current", "w") as f:
            f.write(b"{}")
    else:
        if not fs.exists(f"{commit_sync_conf}/all"):
            fs.cp(f"{test_fixtures}/commitsync/all.json", f"{commit_sync_conf}/all")
        if not fs.exists(f"{commit_sync_conf}/current"):
            fs.cp(
                f"{test_fixtures}/commitsync/current.json",
                f"{commit_sync_conf}/current",
            )

    # Setup XDB GC Config
    xdb_gc_conf = f"{local_configerator_path}/scm/mononoke/xdb_gc"
    env.setenv("XDB_GC_CONF", xdb_gc_conf)
    fs.mkdir(xdb_gc_conf)
    if not fs.exists(f"{xdb_gc_conf}/default"):
        with fs.open(f"{xdb_gc_conf}/default", "w") as f:
            f.write(
                b"""
{
  "put_generation": 2,
  "mark_generation": 1,
  "delete_generation": 0
}
"""
            )

    # Setup Observability Config
    observability_conf = f"{local_configerator_path}/scm/mononoke/observability"
    env.setenv("OBSERVABILITY_CONF", observability_conf)
    fs.mkdir(observability_conf)
    if not fs.exists(f"{observability_conf}/observability_config"):
        with fs.open(f"{observability_conf}/observability_config", "w") as f:
            f.write(
                b"""
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
"""
            )

    # Setup Redaction Config
    redaction_conf = f"{local_configerator_path}/scm/mononoke/redaction"
    env.setenv("REDACTION_CONF", redaction_conf)
    fs.mkdir(redaction_conf)
    if not fs.exists(f"{redaction_conf}/redaction_sets"):
        with fs.open(f"{redaction_conf}/redaction_sets", "w") as f:
            f.write(
                b"""
{
  "all_redactions": []
}
"""
            )

    # Setup Replication Lag Config
    replication_lag_conf = (
        f"{local_configerator_path}/scm/mononoke/mysql/replication_lag/config"
    )
    env.setenv("REPLICATION_LAG_CONF", replication_lag_conf)
    fs.mkdir(replication_lag_conf)
    for conf in ["healer", "derived_data_backfiller", "derived_data_tailer"]:
        if not fs.exists(f"{replication_lag_conf}/{conf}"):
            with fs.open(f"{replication_lag_conf}/{conf}", "w") as f:
                f.write(b"{}")

    return 0


def setup_common_hg_configs(
    fs: ShellFS,
    env: Env,
) -> int:
    hg_rc_path = env.getenv("HGRCPATH")
    test_tmp = env.getenv("TESTTMP")
    test_certdir = env.getenv("TEST_CERTDIR")
    override_client_cert = env.getenv("OVERRIDE_CLIENT_CERT", "client0")
    main_bookmark = env.getenv("MASTER_BOOKMARK", "master_bookmark")

    config_content = f"""
[ui]
ssh="{env.getenv('DUMMYSSH')}"

[devel]
segmented-changelog-rev-compat=True

[extensions]
commitextras=
remotenames=
smartlog=
clienttelemetry=

[remotefilelog]
cachepath={test_tmp}/cachepath
shallowtrees=True

[remotenames]
selectivepulldefault={main_bookmark}

[hint]
ack=*

[experimental]
changegroup3=True

[mutation]
record=False

[web]
cacerts={test_certdir}/root-ca.crt

[auth]
mononoke.prefix=*
mononoke.schemes=https mononoke
mononoke.cert={test_certdir}/{override_client_cert}.crt
mononoke.key={test_certdir}/{override_client_cert}.key
mononoke.cn=localhost

[schemes]
hg=ssh://user@dummy/{{1}}

[cas]
disable=false
use-case=source-control-testing
log-dir={test_tmp}
"""

    with fs.open(hg_rc_path, "a") as f:
        f.write(config_content.encode())

    # Check if the 'mono' scheme is already set
    env.args = ["hg", "config", "schemes.mono"]
    if hgcmd(BufIO.devnull(), BufIO.devnull(), BufIO.devnull(), env) != 0:
        with fs.open(hg_rc_path, "a") as f:
            f.write(
                b"""
[schemes]
mono=ssh://user@dummy/{1}
"""
            )

    return 0


def setup_mononoke_config(
    args: List[str],
    stderr: BinaryIO,
    fs: ShellFS,
    env: Env,
) -> int:
    fs.chdir(env.getenv("TESTTMP"))

    fs.mkdir("mononoke-config")
    repotype = "blob_sqlite"
    if len(args) > 0:
        repotype = args.pop(0)
    env.setenv("REPOTYPE", repotype)

    blobstorename = "blobstore"
    if len(args) > 0:
        blobstorename = args.pop(0)

    fs.chdir("mononoke-config")
    fs.mkdir("common")
    with fs.open("common/common.toml", "w") as f:
        f.write(b"")
    with fs.open("common/commitsyncmap.toml", "w") as f:
        f.write(b"")

    scuba_censored_logging_path = env.getenv("SCUBA_CENSORED_LOGGING_PATH")
    if scuba_censored_logging_path:
        with fs.open("common/common.toml", "w") as f:
            f.write(
                f'scuba_local_path_censored="{scuba_censored_logging_path}"\n'.encode()
            )

    if not env.getenv("DISABLE_HTTP_CONTROL_API"):
        with fs.open("common/common.toml", "a") as f:
            f.write(b"enable_http_control_api=true\n")

    with fs.open("common/common.toml", "a") as f:
        f.write(
            f"""
[async_requests_config]
db_config = {{ local = {{ local_db_path="{env.getenv("TESTTMP")}/monsql" }} }}
blobstore_config = {{ blob_files = {{ path = "{env.getenv("TESTTMP")}/async_requests.blobstore" }} }}

[internal_identity]
identity_type = "SERVICE_IDENTITY"
identity_data = "proxy"

[redaction_config]
blobstore = "{blobstorename}"
redaction_sets_location = "scm/mononoke/redaction/redaction_sets"

[[trusted_parties_allowlist]]
identity_type = "{env.getenv("PROXY_ID_TYPE")}"
identity_data = "{env.getenv("PROXY_ID_DATA")}"
""".encode()
        )

    # Setup Redaction Config
    t = env.getenv("TESTTMP")
    redaction_conf = f"{t}/configerator/scm/mononoke/redaction"
    env.setenv("REDACTION_CONF", redaction_conf)
    fs.mkdir(redaction_conf)
    if not fs.exists(f"{redaction_conf}/redaction_sets"):
        with fs.open(f"{redaction_conf}/redaction_sets", "w") as f:
            f.write(
                b"""
{
  "all_redactions": []
}
"""
            )

    additional_mononoke_common_config = env.getenv("ADDITIONAL_MONONOKE_COMMON_CONFIG")
    if additional_mononoke_common_config:
        with fs.open("common/common.toml", "a") as f:
            f.write(f"{additional_mononoke_common_config}\n".encode())

    with fs.open("common/storage.toml", "w") as f:
        f.write(b"# Start new config\n")

    setup_mononoke_storage_config([repotype, blobstorename], stderr, fs, env)
    setup_mononoke_repo_config(
        [env.getenv("REPONAME", ""), blobstorename], stderr, fs, env
    )
    setup_acls(stderr, fs, env)

    return 0


def setup_acls(stderr: BinaryIO, fs: ShellFS, env: Env) -> int:
    acl_file = env.getenv("ACL_FILE")
    if not acl_file:
        stderr.write(b"Error: ACL_FILE environment variable is not set\n")
        return 1

    # Check if the ACL file already exists
    if not fs.exists(acl_file):
        client0_id_type = env.getenv("CLIENT0_ID_TYPE")
        client0_id_data = env.getenv("CLIENT0_ID_DATA")

        # Create the ACL file with the necessary permissions
        acl_content = {
            "repos": {
                "default": {
                    "actions": {
                        "read": [f"{client0_id_type}:{client0_id_data}"],
                        "write": [f"{client0_id_type}:{client0_id_data}"],
                    }
                }
            }
        }
        with fs.open(acl_file, "w") as f:
            f.write(json.dumps(acl_content).encode())

    return 0


def setup_mononoke_repo_config(
    args: List[str],
    stderr: BinaryIO,
    fs: ShellFS,
    env: Env,
) -> int:
    if len(args) < 1:
        stderr.write(
            b"Error: Not enough arguments provided to setup_mononoke_repo_config\n"
        )
        return 1

    reponame = args[0]
    storageconfig = None if len(args) == 1 else args[1]
    test_tmp = env.getenv("TESTTMP")
    fs.chdir(f"{test_tmp}/mononoke-config")

    reponame_urlencoded = urlencode(["encode", reponame], stderr)
    if reponame_urlencoded == 1:
        return 1
    everstore_local_path = f"{test_tmp}/everstore_{reponame_urlencoded}"

    fs.mkdir(f"repos/{reponame_urlencoded}")
    fs.mkdir(f"repo_definitions/{reponame_urlencoded}")
    fs.mkdir(f"{test_tmp}/monsql")
    fs.mkdir(everstore_local_path)

    repo_config_path = f"repos/{reponame_urlencoded}/server.toml"
    repo_definitions_config_path = f"repo_definitions/{reponame_urlencoded}/server.toml"

    main_bookmark = env.getenv("MASTER_BOOKMARK", "master_bookmark")

    def append_config(content: str, mode: str = "a"):
        with fs.open(repo_config_path, mode) as f:
            f.write((content + "\n").encode())

    def append_def_config(content: str, mode: str = "a"):
        with fs.open(repo_definitions_config_path, mode) as f:
            f.write((content + "\n").encode())

    ## Determine the enabled derived data types
    ## we'll use them later but let's make them visible in entirety of the function
    if enabled_derived_data := env.getenv("ENABLED_DERIVED_DATA"):
        derived_data_types = enabled_derived_data.split()
    else:
        derived_data_types = [
            "blame",
            "changeset_info",
            "deleted_manifest",
            "fastlog",
            "filenodes",
            "fsnodes",
            "git_commits",
            "git_delta_manifests_v2",
            "unodes",
            "hgchangesets",
            "hg_augmented_manifests",
            "skeleton_manifests",
            "skeleton_manifests_v2",
            "bssm_v3",
            "ccsm",
            "test_manifests",
            "test_sharded_manifests",
        ]

    if additional_derived_data := env.getenv("ADDITIONAL_DERIVED_DATA"):
        derived_data_types.extend(additional_derived_data.split())
    if disabled_derived_data := env.getenv("DISABLED_DERIVED_DATA"):
        derived_data_types = list(
            set(derived_data_types) - set(disabled_derived_data.split())
        )

    append_config(
        f"""
hash_validation_percentage=100
everstore_local_path="{everstore_local_path}"
""",
        "w",
    )

    append_def_config(
        f"""
repo_id={env.getenv('REPOID')}
repo_name="{reponame}"
repo_config="{reponame}"
enabled={env.getenv('ENABLED', 'true')}
hipster_acl="{env.getenv('ACL_NAME', 'default')}"
""",
        "w",
    )

    if env.getenv("READ_ONLY_REPO"):
        append_def_config("\nreadonly=true\n")

    if env.getenv("COMMIT_IDENTITY_SCHEME"):
        append_def_config(
            f"\ndefault_commit_identity_scheme={env.getenv('COMMIT_IDENTITY_SCHEME')}\n"
        )
    ## if hgchangesets are not derived let's use git
    elif "hgchangesets" not in derived_data_types:
        append_def_config("\ndefault_commit_identity_scheme=3\n")

    if env.getenv("SCUBA_LOGGING_PATH"):
        append_config(f"\nscuba_local_path=\"{env.getenv('SCUBA_LOGGING_PATH')}\"\n")

    if env.getenv("HOOKS_SCUBA_LOGGING_PATH"):
        append_config(
            f"\nscuba_table_hooks=\"file://{env.getenv('HOOKS_SCUBA_LOGGING_PATH')}\"\n"
        )

    if env.getenv("ENFORCE_LFS_ACL_CHECK"):
        append_config("\nenforce_lfs_acl_check=true\n")

    if env.getenv("REPO_CLIENT_USE_WARM_BOOKMARKS_CACHE"):
        append_config("\nrepo_client_use_warm_bookmarks_cache=true\n")

    if env.getenv("GIT_LFS_INTERPRET_POINTERS") == "1":
        append_config("\ngit_configs.git_lfs_interpret_pointers = true\n")

    # Check if storageconfig is not provided and set up storage config
    if not storageconfig:
        storageconfig = f"blobstore_{reponame_urlencoded}"
        setup_mononoke_storage_config(
            [env.getenv("REPOTYPE"), storageconfig], stderr, fs, env
        )

    append_config(f'\nstorage_config = "{storageconfig}"\n')

    if env.getenv("FILESTORE"):
        filestore_chunk_size = env.getenv("FILESTORE_CHUNK_SIZE", "10")
        append_config(
            f"""
[filestore]
chunk_size = {filestore_chunk_size}
concurrency = 24
"""
        )

    if env.getenv("REDACTION_DISABLED"):
        append_config("redaction=false")

    if env.getenv("LIST_KEYS_PATTERNS_MAX"):
        list_keys_patterns_max = env.getenv("LIST_KEYS_PATTERNS_MAX")
        append_config(f"list_keys_patterns_max={list_keys_patterns_max}")

    if env.getenv("ONLY_FAST_FORWARD_BOOKMARK"):
        only_fast_forward_bookmark = env.getenv("ONLY_FAST_FORWARD_BOOKMARK")
        append_config(
            f"""
[[bookmarks]]
name="{only_fast_forward_bookmark}"
only_fast_forward=true
"""
        )

    if env.getenv("ONLY_FAST_FORWARD_BOOKMARK_REGEX"):
        only_fast_forward_bookmark_regex = env.getenv(
            "ONLY_FAST_FORWARD_BOOKMARK_REGEX"
        )
        append_config(
            f"""
[[bookmarks]]
regex="{only_fast_forward_bookmark_regex}"
only_fast_forward=true
"""
        )

    append_config(
        """
[metadata_logger_config]
bookmarks=["master"]
"""
    )

    append_config(
        f"""
[mononoke_cas_sync_config]
main_bookmark_to_sync="{main_bookmark}"
sync_all_bookmarks=true
"""
    )

    append_config(
        f"""
[modern_sync_config]
url="https://localhost"
chunk_size=100
single_db_query_entries_limit=10
changeset_concurrency=10
max_blob_bytes=1000000
content_channel_config={{ batch_size=10, channel_size=100, flush_interval_ms=1000 }}
filenodes_channel_config={{ batch_size=10, channel_size=100, flush_interval_ms=1000 }}
trees_channel_config={{ batch_size=10, channel_size=100, flush_interval_ms=1000 }}
changesets_channel_config={{ batch_size=10, channel_size=100, flush_interval_ms=1000 }}
"""
    )

    append_config(
        """
[commit_cloud_config]
mocked_employees=["myusername0@fb.com","anotheruser@fb.com"]
disable_interngraph_notification=true
"""
    )

    append_config(
        """
[pushrebase]
forbid_p2_root_rebases=false
"""
    )

    if env.getenv("ALLOW_CASEFOLDING"):
        append_config("casefolding_check=false")

    if env.getenv("BLOCK_MERGES"):
        append_config("block_merges=true")

    if env.getenv("PUSHREBASE_REWRITE_DATES"):
        append_config("rewritedates=true")
    else:
        append_config("rewritedates=false")

    if env.getenv("EMIT_OBSMARKERS"):
        append_config("emit_obsmarkers=true")

    if env.getenv("GLOBALREVS_PUBLISHING_BOOKMARK"):
        globalrevs_publishing_bookmark = env.getenv("GLOBALREVS_PUBLISHING_BOOKMARK")
        append_config(
            f'globalrevs_publishing_bookmark = "{globalrevs_publishing_bookmark}"'
        )

    if env.getenv("GLOBALREVS_SMALL_REPO_ID"):
        globalrevs_small_repo_id = env.getenv("GLOBALREVS_SMALL_REPO_ID")
        append_config(f"globalrevs_small_repo_id = {globalrevs_small_repo_id}")

    if env.getenv("POPULATE_GIT_MAPPING"):
        append_config("populate_git_mapping=true")

    if env.getenv("ALLOW_CHANGE_XREPO_MAPPING_EXTRA"):
        append_config("allow_change_xrepo_mapping_extra=true")

    append_config(
        """
[hook_manager_params]
disable_acl_checker=true"""
    )

    if env.getenv("DISALLOW_NON_PUSHREBASE"):
        append_config(
            """
[push]
pure_push_allowed = false
"""
        )
    else:
        append_config(
            """
[push]
pure_push_allowed = true
"""
        )

    if env.getenv("UNBUNDLE_COMMIT_LIMIT"):
        unbundle_commit_limit = env.getenv("UNBUNDLE_COMMIT_LIMIT")
        append_config(f"unbundle_commit_limit = {unbundle_commit_limit}")

    if env.getenv("CACHE_WARMUP_BOOKMARK"):
        cache_warmup_bookmark = env.getenv("CACHE_WARMUP_BOOKMARK")
        append_config(
            f"""
[cache_warmup]
bookmark="{cache_warmup_bookmark}"
"""
        )
        if env.getenv("CACHE_WARMUP_MICROWAVE"):
            append_config("microwave_preload = true")

    append_config("[lfs]")

    if env.getenv("LFS_THRESHOLD"):
        lfs_threshold = env.getenv("LFS_THRESHOLD")
        lfs_rollout_percentage = env.getenv("LFS_ROLLOUT_PERCENTAGE", "100")
        append_config(
            f"""
threshold={lfs_threshold}
rollout_percentage={lfs_rollout_percentage}
"""
        )

    if env.getenv("LFS_USE_UPSTREAM"):
        append_config("use_upstream_lfs_server = true")

    # Assuming write_infinitepush_config is another function to be called
    if (rv := write_infinitepush_config([reponame], stderr, fs, env)) != 0:
        return rv

    append_config(
        """
[derived_data_config]
enabled_config_name = "default"
scuba_table = "file://{}/derived_data_scuba.json"
""".format(env.getenv("TESTTMP"))
    )

    enabled_derived_data = json.dumps(derived_data_types)
    append_config(
        f"""
[derived_data_config.available_configs.default]
types = {enabled_derived_data}
git_delta_manifest_version = 2
git_delta_manifest_v2_config.max_inlined_object_size = 20
git_delta_manifest_v2_config.max_inlined_delta_size = 20
git_delta_manifest_v2_config.delta_chunk_size = 1000
"""
    )

    if other_derived_data := env.getenv("OTHER_DERIVED_DATA"):
        other_derived_data = json.dumps(other_derived_data.split())
        append_config(
            f"""
[derived_data_config.available_configs.other]
types = {other_derived_data}
"""
        )

    if env.getenv("BLAME_VERSION"):
        blame_version = env.getenv("BLAME_VERSION")
        append_config(f"blame_version = {blame_version}")

    if env.getenv("HG_SET_COMMITTER_EXTRA"):
        append_config("hg_set_committer_extra = true")

    if env.getenv("BACKUP_FROM"):
        backup_from = env.getenv("BACKUP_FROM")
        append_def_config(f'backup_source_repo_name="{backup_from}"')

    append_config(
        f"""
[source_control_service]
permit_writes = {env.getenv('SCS_PERMIT_WRITES', 'true')}
permit_service_writes = {env.getenv('SCS_PERMIT_SERVICE_WRITES', 'true')}
permit_commits_without_parents = {env.getenv('SCS_PERMIT_COMMITS_WITHOUT_PARENTS', 'true')}
"""
    )

    if env.getenv("SPARSE_PROFILES_LOCATION"):
        sparse_profiles_location = env.getenv("SPARSE_PROFILES_LOCATION")
        append_config(
            f"""
[sparse_profiles_config]
sparse_profiles_location="{sparse_profiles_location}"
"""
        )

    if (
        env.getenv("COMMIT_SCRIBE_CATEGORY")
        or env.getenv("BOOKMARK_SCRIBE_CATEGORY")
        or env.getenv("GIT_CONTENT_REFS_SCRIBE_CATEGORY")
    ):
        append_config("[update_logging_config]")

    if env.getenv("BOOKMARK_SCRIBE_CATEGORY"):
        bookmark_scribe_category = env.getenv("BOOKMARK_SCRIBE_CATEGORY")
        append_config(
            f"""
bookmark_logging_destination = {{ scribe = {{ scribe_category = "{bookmark_scribe_category}" }} }}
"""
        )

    if env.getenv("COMMIT_SCRIBE_CATEGORY"):
        commit_scribe_category = env.getenv("COMMIT_SCRIBE_CATEGORY")
        append_config(
            f"""
new_commit_logging_destination = {{ scribe = {{ scribe_category = "{commit_scribe_category}" }} }}
"""
        )

    if env.getenv("GIT_CONTENT_REFS_SCRIBE_CATEGORY"):
        git_content_refs_scribe_category = env.getenv(
            "GIT_CONTENT_REFS_SCRIBE_CATEGORY"
        )
        append_config(
            f"""
git_content_refs_logging_destination = {{ scribe = {{ scribe_category = "{git_content_refs_scribe_category}" }} }}
"""
        )

    if env.getenv("ZELOS_PORT"):
        zelos_port = env.getenv("ZELOS_PORT")
        append_config(
            f"""
[zelos_config]
local_zelos_port = {zelos_port}
"""
        )
    if not env.getenv("HGTEST_IS_PROD_MONONOKE"):
        append_config("""
[git_configs.git_bundle_uri_config.uri_generator_type.local_fs]
""")

    if (
        env.getenv("WBC_SCRIBE_CATEGORY")
        or env.getenv("TAGS_SCRIBE_CATEGORY")
        or env.getenv("CONTENT_REFS_SCRIBE_CATEGORY")
    ):
        append_config("[metadata_cache_config]")

    if env.getenv("WBC_SCRIBE_CATEGORY"):
        wbc_scribe_category = env.getenv("WBC_SCRIBE_CATEGORY")
        append_config(
            f"""
wbc_update_mode = {{ tailing = {{ category = "{wbc_scribe_category}" }} }}
"""
        )

    if env.getenv("TAGS_SCRIBE_CATEGORY"):
        tags_scribe_category = env.getenv("TAGS_SCRIBE_CATEGORY")
        append_config(
            f"""
tags_update_mode = {{ tailing = {{ category = "{tags_scribe_category}" }} }}
"""
        )

    if env.getenv("CONTENT_REFS_SCRIBE_CATEGORY"):
        content_refs_scribe_category = env.getenv("CONTENT_REFS_SCRIBE_CATEGORY")
        append_config(
            f"""
content_refs_update_mode = {{ tailing = {{ category = "{content_refs_scribe_category}" }} }}
"""
        )

    return 0


def write_infinitepush_config(
    args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env
) -> int:
    if len(args) < 1:
        stderr.write(
            "Error: Not enough arguments provided to write_infinitepush_config\n"
        )
        return 1
    reponame = args[0]
    reponame_urlencoded = urlencode(["encode", reponame], stderr)
    if reponame_urlencoded == 1:
        return 1
    repo_config_path = f"repos/{reponame_urlencoded}/server.toml"

    # Helper function to append configuration
    def append_config(content: str):
        with fs.open(repo_config_path, "a") as file:
            file.write((content + "\n").encode())

    # Start infinitepush configuration
    append_config("[infinitepush]")
    # Conditional configurations for infinitepush
    infinitepush_allow_writes = env.getenv("INFINITEPUSH_ALLOW_WRITES")
    infinitepush_namespace_regex = env.getenv("INFINITEPUSH_NAMESPACE_REGEX")
    if infinitepush_allow_writes or infinitepush_namespace_regex:
        namespace_config = (
            f'namespace_pattern="{infinitepush_namespace_regex}"'
            if infinitepush_namespace_regex
            else ""
        )
        append_config(f"""
allow_writes = {infinitepush_allow_writes or "true"}
{namespace_config}
""")
    return 0


def urlencode(args: List[str], stderr: BinaryIO) -> Union[str, int]:
    if len(args) < 2:
        stderr.write(b"Error: Not enough arguments provided to urlencode\n")
        return 1

    if args[0] == "decode":
        return unquote(args[1])
    elif args[0] == "encode":
        return quote(args[1], safe="")
    else:
        stderr.write(b"argv[1] must be either 'decode' or 'encode'.\n")
        return 1

    return ""


def setup_mononoke_storage_config(
    args: List[str], stderr: BinaryIO, fs: ShellFS, env: Env
) -> int:
    if len(args) < 2:
        stderr.write(
            "Error: Not enough arguments provided to setup_mononoke_storage_config\n"
        )
        return 1

    underlying_storage = args[0]
    blobstore_name = args[1]
    test_tmp = env.getenv("TESTTMP")
    blobstore_path = f"{test_tmp}/{blobstore_name}"

    bubble_deletion_mode = env.getenv("BUBBLE_DELETION_MODE", "0")
    bubble_lifespan_secs = env.getenv("BUBBLE_LIFESPAN_SECS", "1000")
    bubble_expiration_secs = env.getenv("BUBBLE_EXPIRATION_SECS", "1000")

    multiplexed = env.getenv("MULTIPLEXED")
    if multiplexed:
        quorum = "write_quorum"
        btype = "multiplexed_wal"
        scuba = (
            f'multiplex_scuba_table = "file://{test_tmp}/blobstore_trace_scuba.json"'
        )
        storage_config = f"""{db_config([blobstore_name], stderr, env)}
[{blobstore_name}.blobstore.{btype}]
multiplex_id = 1
{blobstore_db_config(fs, env)}
{quorum} = {multiplexed}
{scuba}
components = [
"""
        for i in range(int(multiplexed) + 1):
            fs.mkdir(f"{blobstore_path}/{i}/blobs")
            if env.getenv("PACK_BLOB") and i <= int(env.getenv("PACK_BLOB")):
                storage_config += f'  {{ blobstore_id = {i}, blobstore = {{ pack = {{ blobstore = {{ {underlying_storage} = {{ path = "{blobstore_path}/{i}" }} }} }} }} }},\n'
            else:
                storage_config += f'  {{ blobstore_id = {i}, blobstore = {{ {underlying_storage} = {{ path = "{blobstore_path}/{i}" }} }} }},\n'
        storage_config += "]\n"
    else:
        fs.mkdir(f"{blobstore_path}/blobs")
        ephem_db_cfg = ephemeral_db_config(
            [f"{blobstore_name}.ephemeral_blobstore"], stderr, env
        )
        storage_config = f"""{db_config([blobstore_name], stderr, env)}
[{blobstore_name}.ephemeral_blobstore]
initial_bubble_lifespan_secs = {bubble_lifespan_secs}
bubble_expiration_grace_secs = {bubble_expiration_secs}
bubble_deletion_mode = {bubble_deletion_mode}
blobstore = {{ blob_files = {{ path = "{blobstore_path}" }} }}

{ephem_db_cfg}

[{blobstore_name}.blobstore]
"""
        if env.getenv("PACK_BLOB"):
            storage_config += f'  pack = {{ blobstore = {{ {underlying_storage} = {{ path = "{blobstore_path}" }} }} }}\n'
        else:
            storage_config += (
                f'  {underlying_storage} = {{ path = "{blobstore_path}" }}\n'
            )

    with fs.open("common/storage.toml", "a") as f:
        f.write(storage_config.encode())

    return 0


def ephemeral_db_config(args: List[str], stderr: BinaryIO, env: Env) -> str:
    if len(args) < 1:
        stderr.write(b"Error: No blobstore name provided to ephemeral_db_config\n")
        return ""

    blobstore_name = args[0]
    db_shard_name = env.getenv("DB_SHARD_NAME")
    test_tmp = env.getenv("TESTTMP")

    if db_shard_name:
        return f"""
[{blobstore_name}.metadata.remote]
db_address = "{db_shard_name}"
"""
    else:
        return f"""
[{blobstore_name}.metadata.local]
local_db_path = "{test_tmp}/monsql"
"""


def db_config(args: List[str], stderr: BinaryIO, env: Env) -> str:
    if len(args) < 1:
        stderr.write(b"Error: No blobstore name provided to db_config\n")
        return ""

    blobstore_name = args[0]
    db_shard_name = env.getenv("DB_SHARD_NAME")
    test_tmp = env.getenv("TESTTMP")

    if db_shard_name:
        return f"""[{blobstore_name}.metadata.remote]
primary = {{ db_address = "{db_shard_name}" }}
filenodes = {{ unsharded = {{ db_address = "{db_shard_name}" }} }}
mutation = {{ db_address = "{db_shard_name}" }}
commit_cloud = {{ db_address = "{db_shard_name}" }}
git_bundles = {{ db_address = "{db_shard_name}" }}
"""
    else:
        return f"""[{blobstore_name}.metadata.local]
local_db_path = "{test_tmp}/monsql"
"""


def blobstore_db_config(fs: ShellFS, env: Env) -> str:
    db_shard_name = env.getenv("DB_SHARD_NAME")
    test_tmp = env.getenv("TESTTMP")

    if db_shard_name:
        return f"""
queue_db = {{ unsharded = {{ db_address = "{db_shard_name}" }} }}"""
    else:
        blobstore_db_path = f"{test_tmp}/blobstore_sync_queue"
        fs.mkdir(blobstore_db_path)
        return f"""
queue_db = {{ local = {{ local_db_path = "{blobstore_db_path}" }} }}"""


def setup_environment_variables(stderr: BinaryIO, fs: ShellFS, env: Env) -> int:
    if env.getenv("FB_TEST_FIXTURES"):
        env.setenv("HAS_FB", "1")
        env.setenv("FB_PROXY_ID_TYPE", "SERVICE_IDENTITY")
        env.setenv("FB_PROXY_ID_DATA", "proxy")
        env.setenv("FB_CLIENT0_ID_TYPE", "USER")
        env.setenv("FB_CLIENT0_ID_DATA", "myusername0")
        env.setenv("FB_CLIENT1_ID_TYPE", "USER")
        env.setenv("FB_CLIENT1_ID_DATA", "myusername1")
        env.setenv("FB_CLIENT2_ID_TYPE", "USER")
        env.setenv("FB_CLIENT2_ID_DATA", "myusername2")
        env.setenv(
            "FB_JSON_CLIENT_ID",
            '["MACHINE:devvm000.lla0.facebook.com", "MACHINE_TIER:devvm", "USER:myusername0"]',
        )

        env.setenv("SILENCE_SR_DEBUG_PERF_WARNING", "1")
        env.setenv("ENABLE_LOCAL_CACHE", "1")
        env.setenv(
            "MONONOKE_INTEGRATION_TEST_EXPECTED_THRIFT_SERVER_IDENTITY",
            "MACHINE:mononoke-test-server-000.vll0.facebook.com",
        )

        env.setenv("MONONOKE_INTEGRATION_TEST_DISABLE_SR", "true")

    # Setting up proxy and client identity types and data
    env.setenv("PROXY_ID_TYPE", env.getenv("FB_PROXY_ID_TYPE", "X509_SUBJECT_NAME"))
    env.setenv(
        "PROXY_ID_DATA",
        env.getenv("FB_PROXY_ID_DATA", "CN=proxy,O=Mononoke,C=US,ST=CA"),
    )
    env.setenv("CLIENT0_ID_TYPE", env.getenv("FB_CLIENT0_ID_TYPE", "X509_SUBJECT_NAME"))
    env.setenv(
        "CLIENT0_ID_DATA",
        env.getenv("FB_CLIENT0_ID_DATA", "CN=client0,O=Mononoke,C=US,ST=CA"),
    )
    env.setenv("CLIENT1_ID_TYPE", env.getenv("FB_CLIENT1_ID_TYPE", "X509_SUBJECT_NAME"))
    env.setenv(
        "CLIENT1_ID_DATA",
        env.getenv("FB_CLIENT1_ID_DATA", "CN=client1,O=Mononoke,C=US,ST=CA"),
    )
    env.setenv("CLIENT2_ID_TYPE", env.getenv("FB_CLIENT2_ID_TYPE", "X509_SUBJECT_NAME"))
    env.setenv(
        "CLIENT2_ID_DATA",
        env.getenv("FB_CLIENT2_ID_DATA", "CN=client2,O=Mononoke,C=US,ST=CA"),
    )
    env.setenv(
        "JSON_CLIENT_ID",
        env.getenv(
            "FB_JSON_CLIENT_ID",
            '["X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]',
        ),
    )

    # Setting up the scribe logs directory
    test_tmp = env.getenv("TESTTMP")
    scribe_logs_dir = f"{test_tmp}/scribe_logs"
    env.setenv("SCRIBE_LOGS_DIR", scribe_logs_dir)

    # Setting up various timeouts based on the presence of DB_SHARD_NAME
    db_shard_name = env.getenv("DB_SHARD_NAME")
    if db_shard_name:
        env.setenv("MONONOKE_DEFAULT_START_TIMEOUT", "600")
        env.setenv("MONONOKE_LFS_DEFAULT_START_TIMEOUT", "60")
        env.setenv("MONONOKE_GIT_SERVICE_DEFAULT_START_TIMEOUT", "60")
        env.setenv("MONONOKE_SCS_DEFAULT_START_TIMEOUT", "300")
        env.setenv("MONONOKE_LAND_SERVICE_DEFAULT_START_TIMEOUT", "120")
    else:
        env.setenv("MONONOKE_DEFAULT_START_TIMEOUT", "60")
        env.setenv("MONONOKE_LFS_DEFAULT_START_TIMEOUT", "60")
        env.setenv("MONONOKE_GIT_SERVICE_DEFAULT_START_TIMEOUT", "60")
        env.setenv("MONONOKE_SCS_DEFAULT_START_TIMEOUT", "300")
        env.setenv("MONONOKE_LAND_SERVICE_DEFAULT_START_TIMEOUT", "120")
        env.setenv("MONONOKE_DDS_DEFAULT_START_TIMEOUT", "120")

    env.setenv("VI_SERVICE_DEFAULT_START_TIMEOUT", "60")

    # Set initial variables
    env.setenv("REPOID", "0")
    env.setenv("REPONAME", env.getenv("REPONAME", "repo"))

    # Define file paths
    test_tmp = env.getenv("TESTTMP")
    mononoke_server_addr_file = f"{test_tmp}/mononoke_server_addr.txt"
    dds_server_addr_file = f"{test_tmp}/dds_server_addr.txt"
    local_configerator_path = f"{test_tmp}/configerator"
    acl_file = f"{test_tmp}/acls.json"

    # Export environment variables
    env.setenv("MONONOKE_SERVER_ADDR_FILE", mononoke_server_addr_file)
    env.setenv("DDS_SERVER_ADDR_FILE", dds_server_addr_file)
    env.setenv("LOCAL_CONFIGERATOR_PATH", local_configerator_path)
    env.setenv("ACL_FILE", acl_file)

    # Create directories
    fs.mkdir(local_configerator_path)

    # Copy default knobs file
    just_knobs_defaults = env.getenv("JUST_KNOBS_DEFAULTS")
    mononoke_just_knobs_overrides_path = f"{local_configerator_path}/just_knobs.json"
    env.setenv("MONONOKE_JUST_KNOBS_OVERRIDES_PATH", mononoke_just_knobs_overrides_path)
    fs.cp(
        f"{just_knobs_defaults}/just_knobs_defaults/just_knobs.json",
        mononoke_just_knobs_overrides_path,
    )

    # Setup cache arguments based on ENABLE_LOCAL_CACHE
    enable_local_cache = env.getenv("ENABLE_LOCAL_CACHE")
    if enable_local_cache:
        cache_args = [
            "--cache-mode=local-only",
            "--cache-size-gb=1",
            "--cachelib-disable-cacheadmin",
        ]
    else:
        cache_args = ["--cache-mode=disabled"]
    all_args = " ".join(cache_args)
    env.setenv("CACHE_ARGS_U", all_args)
    env.setenv("CACHE_ARGS", "(" + all_args + ")")

    # Common arguments
    common_args = [
        "--mysql-master-only",
        "--just-knobs-config-path",
        get_configerator_relative_path(mononoke_just_knobs_overrides_path, env),
        "--local-configerator-path",
        local_configerator_path,
        "--log-exclude-tag",
        "futures_watchdog",
        "--with-test-megarepo-configs-client=true",
        "--acl-file",
        acl_file,
        "--runtime-threads",
        "6",
    ]
    all_args = " ".join(common_args)
    env.setenv("COMMON_ARGS_U", all_args)
    env.setenv("COMMON_ARGS", "(" + " ".join(common_args) + ")")

    # Set up certificate directory
    hgtest_certdir = env.getenv("HGTEST_CERTDIR")
    test_certs = env.getenv("TEST_CERTS")
    test_certdir = env.getenv(
        "TEST_CERTDIR", hgtest_certdir if hgtest_certdir else test_certs
    )
    if not test_certdir:
        stderr.write(b"TEST_CERTDIR is not set\n")
        return 1
    env.setenv("TEST_CERTDIR", test_certdir)

    return 0


def get_configerator_relative_path(
    target_path: str,
    env: Env,
) -> str:
    local_configerator_path = env.getenv("LOCAL_CONFIGERATOR_PATH")

    path = os.path.realpath(target_path)
    base = os.path.realpath(local_configerator_path)

    common_prefix = os.path.commonpath([path, base])

    if common_prefix == base:
        relative_path = os.path.relpath(path, base)
    else:
        # If there's no common prefix, return the absolute path
        relative_path = path

    return relative_path
