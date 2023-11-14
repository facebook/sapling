# This needs to use native. to define a UDR.
# @lint-ignore-every BUCKLINT

load("@fbcode_macros//build_defs:custom_rule.bzl", "custom_rule")
load("@fbcode_macros//build_defs:custom_unittest.bzl", "custom_unittest")
load("@fbcode_macros//build_defs:native_rules.bzl", "buck_filegroup")
load("@fbcode_macros//build_defs/lib:rust_common.bzl", "rust_common")
load("@fbcode_macros//build_defs/lib:rust_oss.bzl", "rust_oss")
load("@fbsource//tools/build_defs/buck2:is_buck2.bzl", "is_buck2")

MONONOKE_TARGETS_TO_ENV = {
    "//common/tools/thriftdbg:thriftdbg": "THRIFTDBG",  # Used for verify_integrity_service health check
    "//eden/mononoke/benchmarks/filestore:benchmark_filestore": "MONONOKE_BENCHMARK_FILESTORE",
    "//eden/mononoke/cmds/copy_blobstore_keys:copy_blobstore_keys": "COPY_BLOBSTORE_KEYS",
    "//eden/mononoke/commit_rewriting/backsyncer:backsyncer_cmd": "BACKSYNCER",
    "//eden/mononoke/commit_rewriting/commit_validator:commit_validator": "COMMIT_VALIDATOR",
    "//eden/mononoke/commit_rewriting/megarepo:megarepotool": "MEGAREPO_TOOL",
    "//eden/mononoke/commit_rewriting/mononoke_x_repo_sync_job:mononoke_x_repo_sync_job": "MONONOKE_X_REPO_SYNC",
    "//eden/mononoke/facebook/bookmark_service:bookmark_service_client_cli": "MONONOKE_BOOKMARK_SERVICE_CLIENT",
    "//eden/mononoke/facebook/bookmark_service:bookmark_service_server": "MONONOKE_BOOKMARK_SERVICE_SERVER",
    "//eden/mononoke/facebook/derived_data_service:2ds_client": "DERIVED_DATA_CLIENT",
    "//eden/mononoke/facebook/derived_data_service:derivation_worker": "DERIVED_DATA_WORKER",
    "//eden/mononoke/facebook/derived_data_service:derived_data_service": "DERIVED_DATA_SERVICE",
    "//eden/mononoke/facebook/mirror_hg_commits:mirror_hg_commits": "MIRROR_HG_COMMITS",
    "//eden/mononoke/facebook/slow_bookmark_mover:slow_bookmark_mover": "MONONOKE_SLOW_BOOKMARK_MOVER",
    "//eden/mononoke/git/facebook/git_move_bookmark:git_move_bookmark": "MONONOKE_GIT_MOVE_BOOKMARK",
    "//eden/mononoke/git/facebook/remote_gitimport:remote_gitimport": "MONONOKE_REMOTE_GITIMPORT",
    "//eden/mononoke/git/gitexport:gitexport": "MONONOKE_GITEXPORT",
    "//eden/mononoke/git/gitimport:gitimport": "MONONOKE_GITIMPORT",
    "//eden/mononoke/land_service:land_service": "LAND_SERVICE",
    "//eden/mononoke/lfs_server:lfs_server": "LFS_SERVER",
    "//eden/mononoke/microwave:builder": "MONONOKE_MICROWAVE_BUILDER",
    "//eden/mononoke/mononoke_hg_sync_job:mononoke_hg_sync_job": "MONONOKE_HG_SYNC",
    "//eden/mononoke/repo_import:repo_import": "MONONOKE_REPO_IMPORT",
    "//eden/mononoke/scs/client:scsc": "SCS_CLIENT",
    "//eden/mononoke/scs_server:scs_server": "SCS_SERVER",
    "//eden/mononoke/streaming_clone:new_streaming_clone": "MONONOKE_STREAMING_CLONE",
    "//eden/mononoke/tools/admin:newadmin": "MONONOKE_NEWADMIN",
    "//eden/mononoke/tools/facebook/backfill_bonsai_blob_mapping:backfill_bonsai_blob_mapping": "MONONOKE_BACKFILL_BONSAI_BLOB_MAPPING",
    "//eden/mononoke/tools/import:import": "MONONOKE_IMPORT",
    "//eden/mononoke/tools/testtool:testtool": "MONONOKE_TESTTOOL",
    "//eden/mononoke/walker:walker": "MONONOKE_WALKER",
    "//eden/mononoke:admin": "MONONOKE_ADMIN",
    "//eden/mononoke:aliasverify": "MONONOKE_ALIAS_VERIFY",
    "//eden/mononoke:backfill_derived_data": "MONONOKE_BACKFILL_DERIVED_DATA",
    "//eden/mononoke:backfill_mapping": "MONONOKE_BACKFILL_MAPPING",
    "//eden/mononoke:blobimport": "MONONOKE_BLOBIMPORT",
    "//eden/mononoke:blobstore_healer": "MONONOKE_BLOBSTORE_HEALER",
    "//eden/mononoke:bonsai_verify": "MONONOKE_BONSAI_VERIFY",
    "//eden/mononoke:check_git_wc": "MONONOKE_CHECK_GIT_WC",
    "//eden/mononoke:mononoke": "MONONOKE_SERVER",
    "//eden/mononoke:packer": "MONONOKE_PACKER",
    "//eden/mononoke:segmented_changelog_seeder": "MONONOKE_SEGMENTED_CHANGELOG_SEEDER",
    "//eden/mononoke:segmented_changelog_tailer": "MONONOKE_SEGMENTED_CHANGELOG_TAILER",
    "//eden/mononoke:sqlblob_gc": "MONONOKE_SQLBLOB_GC",
    "//security/source_control/verify_integrity/service:verify_integrity_service": "VERIFY_INTEGRITY_SERVICE",
    "//security/source_control/verify_integrity:verify_integrity": "VERIFY_INTEGRITY",
    "//signedsources:fixtures": "SIGNED_SOURCES_FIXTURES",
    "//zeus/zelos/interactive_cli:zeloscli": "ZELOSCLI",
}

# Every .t test run needs these currently
DOTT_DEPS = {
    "//eden/mononoke/tests/integration/certs/facebook:test_certs": "TEST_CERTS",
    # fixtures
    "//eden/mononoke/tests/integration/facebook:facebook_test_fixtures": "FB_TEST_FIXTURES",
    # Location ofthe .t tests
    "//eden/mononoke/tests/integration/facebook:facebook_tests": "TEST_ROOT_FACEBOOK",
    # Test utils
    "//eden/mononoke/tests/integration:get_free_socket": "GET_FREE_SOCKET",
    "//eden/mononoke/tests/integration:public_tests": "TEST_ROOT_PUBLIC",
    "//eden/mononoke/tests/integration:test_fixtures": "TEST_FIXTURES",
    "//eden/mononoke/tests/integration:urlencode": "URLENCODE",
    "//eden/scm/tests:dummyssh3": "DUMMYSSH",
    # The underlying hg test runner code we depend upon
    "//eden/scm/tests:test_runner": "RUN_TESTS_LIBRARY",
    # The hg build
    "//eden/scm:hg": "BINARY_HG",
    # The version of python to run
    "//eden/scm:hgpython": "BINARY_HGPYTHON",
}

DISABLE_ALL_NETWORK_ACCESS_DEPS = {
    # Stop network
    "//eden/mononoke/tests/integration/facebook:disable-all-network-access": "DISABLE_ALL_NETWORK_ACCESS",
}

# These are used for buck's @mode/dev-rust-oss builds
# The "//" in the values here corresponds to the root of repo (both GitHub and
# fbcode repos have the same folder layout)
# Use None as value to explicitly remove a dependency.  /facebook: dependencies are auto-removed
OSS_DEPS_REPLACEMENTS = {
    "TEST_CERTS": "//eden/mononoke/tests/integration/certs:oss_test_certs",
}

def _generate_manifest_impl(ctx):
    out = ctx.actions.declare_output(ctx.attrs.filename)
    ctx.actions.run(
        [ctx.attrs.generator[native.RunInfo], out.as_output()] + list(ctx.attrs.env.keys()),
        env = {k: native.cmd_args(v).ignore_artifacts() for (k, v) in ctx.attrs.env.items()},
        category = "manifest",
        identifier = ctx.attrs.filename,
    )
    return [native.DefaultInfo(
        default_outputs = [out],
        sub_targets = {
            "deps": [
                native.DefaultInfo(other_outputs = [native.cmd_args(list(ctx.attrs.env.values()))]),
            ],
        },
    )]

generate_manifest = native.rule(
    impl = _generate_manifest_impl,
    attrs = {
        "env": native.attrs.dict(
            key = native.attrs.string(),
            value = native.attrs.arg(),
        ),
        "filename": native.attrs.string(),
        "generator": native.attrs.exec_dep(),
    },
) if is_buck2() else None

def custom_manifest_rule(name, manifest_file, targets):
    if rust_oss.is_oss_build():
        to_remove = []

        # do any replacements or explicitly removals needed
        for k, replacement in OSS_DEPS_REPLACEMENTS.items():
            if k in targets:
                if replacement:
                    targets[k] = replacement
                elif k in targets:
                    to_remove.append(k)

        for k, v in targets.items():
            # remove fb internal targets
            if "/facebook:" in v:
                to_remove.append(k)

        for k in to_remove:
            targets.pop(k)

    env = {k: "$(location %s)" % v for k, v in targets.items()}

    if is_buck2():
        generate_manifest(
            name = name,
            generator = "//eden/mononoke/tests/integration/facebook:generate_manifest",
            env = env,
            filename = manifest_file,
        )
    else:
        custom_rule(
            name = name,
            add_install_dir = False,
            build_args = " ".join([manifest_file] + list(targets.keys())),
            build_script_dep = "//eden/mononoke/tests/integration/facebook:generate_manifest",
            env = env,
            output_gen_files = [manifest_file],
            strict = True,
        )

    return list(targets.values())

def dott_test(name, dott_files, deps, use_mysql = False, disable_all_network_access_target = True):
    _dott_test(name, dott_files, deps, use_mysql, False)

    if use_mysql:
        # NOTE: We need network to talk to MySQL
        disable_all_network_access_target = False

    if disable_all_network_access_target:
        # there's not much sense in blocking network for OSS builds
        _dott_test(name + "-disable-all-network-access", dott_files, deps, use_mysql, disable_all_network_access = True, rust_allow_oss_build = False)

def _dott_test(name, dott_files, deps, use_mysql = False, disable_all_network_access = True, rust_allow_oss_build = None):
    manifest_target = name + "-manifest"

    noop_for_oss = rust_common.is_noop_in_oss_build(rust_allow_oss_build)

    if noop_for_oss:
        rust_common.make_noop_oss_build_rule(
            name = name,
            visibility = ["PUBLIC"],
            executable = True,
        )
        rust_common.make_noop_oss_build_rule(
            name = name + "-dott",
            visibility = ["PUBLIC"],
            executable = False,
        )

        rust_common.make_noop_oss_build_rule(
            name = name + "-manifest",
            visibility = ["PUBLIC"],
            executable = False,
        )
        return

    targets = {}
    for d in deps:
        # test runner takes sybolic names not targets, map from targets to the placeholder names
        if d in DOTT_DEPS:
            env_name = DOTT_DEPS[d]
            targets[env_name] = d
            continue

        if d not in MONONOKE_TARGETS_TO_ENV:
            fail("Unknown target", d, "in dependencies for", name)

        env_name = MONONOKE_TARGETS_TO_ENV[d]
        targets[env_name] = d

    # make sure we have all the mandatory stuff the runner requires
    for t, e in DOTT_DEPS.items():
        if t not in targets:
            targets[e] = t

    if disable_all_network_access:
        for t, e in DISABLE_ALL_NETWORK_ACCESS_DEPS.items():
            if t not in targets:
                targets[e] = t

    # the custom_manifest_rule replaces some deps, e.g. for OSS builds
    resolved_deps = custom_manifest_rule(manifest_target, name + "-manifest.json", targets)
    resolved_deps.append(":" + manifest_target)

    dott_files_target = name + "-dott"

    buck_filegroup(
        name = dott_files_target,
        srcs = dott_files,
    )

    resolved_deps.append(":" + dott_files_target)

    extra_args = [
        arg
        for pair in [["--discovered-test", t] for t in dott_files]
        for arg in pair
    ]

    if use_mysql:
        extra_args.extend([
            "--mysql-client",
            "--mysql-schema",
            "scm/mononoke/mysql/xdb.mononoke_production",
            "--mysql-schema",
            "scm/mononoke/mysql/xdb.mononoke_mutation",
            "--mysql-schema",
            "scm/mononoke/mysql/xdb.mononoke_blobstore_wal_queue",
        ])

    # and now the actual test
    custom_unittest(
        name = name,
        command = [
            "$(location //eden/mononoke/tests/integration:integration_runner_real)",
            "$(location :%s)" % manifest_target,
        ] + extra_args,
        env = {
            "NO_LOCAL_PATHS": "1",
        },

        # This is not really a junit test. It pretends to be one for testpilot. For
        # tpx we want to do better, override the "test type" through a label to
        # work with both testpilot and tpx for now.
        tags = ["tpx-test-type:mononoke_integration"],
        type = "junit",
        deps = resolved_deps,
    )
