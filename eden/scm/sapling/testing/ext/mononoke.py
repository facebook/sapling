# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import BinaryIO, List, Union
from urllib.parse import quote, unquote

from ..sh.interp import interpcode

from ..sh.types import Env, ShellFS
from ..t.runtime import TestTmp


def testsetup(t: TestTmp):
    setupfuncs(t)
    t.sheval("setup_environment_variables")


def setupfuncs(t: TestTmp):
    t.command(setup_mononoke_repo_config)
    t.command(write_infinitepush_config)
    t.command(urlencode)
    t.command(setup_mononoke_storage_config)
    t.command(ephemeral_db_config)
    t.command(db_config)
    t.command(blobstore_db_config)
    t.command(setup_environment_variables)


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

    def append_config(content: str, mode: str = "a"):
        with fs.open(repo_config_path, mode) as f:
            f.write((content + "\n").encode())

    def append_def_config(content: str, mode: str = "a"):
        with fs.open(repo_definitions_config_path, mode) as f:
            f.write((content + "\n").encode())

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
        """
[mononoke_cas_sync_config]
main_bookmark_to_sync="master_bookmark"
sync_all_bookmarks=true
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
        lfs_blob_hg_sync_job = env.getenv("LFS_BLOB_HG_SYNC_JOB", "true")
        append_config(
            f"""
threshold={lfs_threshold}
rollout_percentage={lfs_rollout_percentage}
generate_lfs_blob_in_hg_sync_job={lfs_blob_hg_sync_job}
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

    if env.getenv("ENABLED_DERIVED_DATA"):
        enabled_derived_data = env.getenv("ENABLED_DERIVED_DATA")
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
    elif env.getenv("NON_GIT_TYPES"):
        append_config(
            f"""
[derived_data_config.available_configs.default]
types=[
  "blame",
  "changeset_info",
  "deleted_manifest",
  "fastlog",
  "filenodes",
  "fsnodes",
  "unodes",
  "hgchangesets",
  "hg_augmented_manifests",
  "skeleton_manifests",
  "skeleton_manifests_v2",
  "bssm_v3",
  "ccsm",
  "test_manifests",
  "test_sharded_manifests"
]
"""
        )
    else:
        append_config(
            """
[derived_data_config.available_configs.default]
types=[
  "blame",
  "changeset_info",
  "deleted_manifest",
  "fastlog",
  "filenodes",
  "fsnodes",
  "git_commits",
  "git_delta_manifests_v2",
  "git_trees",
  "unodes",
  "hgchangesets",
  "hg_augmented_manifests",
  "skeleton_manifests",
  "skeleton_manifests_v2",
  "bssm_v3",
  "ccsm",
  "test_manifests",
  "test_sharded_manifests"
]
git_delta_manifest_version = 2
git_delta_manifest_v2_config.max_inlined_object_size = 20
git_delta_manifest_v2_config.max_inlined_delta_size = 20
git_delta_manifest_v2_config.delta_chunk_size = 1000
"""
        )

    if env.getenv("OTHER_DERIVED_DATA"):
        other_derived_data = env.getenv("OTHER_DERIVED_DATA")
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

    if env.getenv("COMMIT_SCRIBE_CATEGORY") or env.getenv("BOOKMARK_SCRIBE_CATEGORY"):
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

    if env.getenv("ZELOS_PORT"):
        zelos_port = env.getenv("ZELOS_PORT")
        append_config(
            f"""
[zelos_config]
local_zelos_port = {zelos_port}
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
    env.setenv("CACHE_ARGS", "(" + " ".join(cache_args) + ")")

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
    ]
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
