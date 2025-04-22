load("@fbcode_macros//build_defs:native_rules.bzl", "buck_command_alias")
load("@fbcode_macros//build_defs:python_unittest.bzl", "python_unittest")
load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")
load("//eden:defs.bzl", "get_integration_test_env_and_deps")

def excluded_t_tests():
    excluded = []
    if read_bool("fbcode", "mode_win_enabled", False):
        excluded += [
            # debugurntest under buck has issues with persisting some files on these tests
            "commitcloud_sync_bookmarks_t",
            "copytrace_rebase_renamed_t",
            "debugcheckcasecollisions_treemanifest_t",
            "debugstrip_t",
            "fb_ext_crdump_t",
            "fb_ext_drop_t",
            "fb_ext_mergedriver_t",
            "fb_ext_pull_createmarkers_check_local_versions_t",
            "fb_ext_pushrebase_remotenames_t",
            "histedit_mutation_t",
            "infinitepush_scratchbookmark_commands_t",
            "network_doctor_t",
            "remotenames_fastheaddiscovery_hidden_commits_t",

            # see comment in core.tests.blacklist
            "help_t",

            # non-debugruntest tests do not work under buck for the most part,
            # as sandcastle has an even older version of /bin/bash and other commands
            # and .bat file shenanigans
            "debugrefreshconfig_t",
            "default_command_t",
            "fb_ext_fastlog_t",
            "fb_ext_remotefilelog_pull_noshallow_t",
            "histedit_edit_t",
            "histedit_no_change_t",
            "import_eol_t",
            "infinitepush_remotenames_t",
            "merge_halt_t",
            "patch_offset_t",
            "pull_pull_corruption_t",
            "rebase_inmemory_noconflict_t",
            "rebase_templates_t",
            "rename_t",
            "rust_clone_t",
            "share_t",
            "sparse_hgrc_profile_t",

            # times out
            "commitcloud_sync_t",
            "fb_ext_copytrace_t",
            "merge_changedelete_t",
        ]
    return excluded

def excluded_watchman_t_tests():
    excluded = [
        # times out
        "merge_tools_t",
        "shelve_t",
        "import_t",
    ]
    if read_bool("fbcode", "mode_win_enabled", False):
        excluded += [
            # debugruntest issues (see comment in excluded_t_tests)
            "pushrebase_withmerges_t",
            "treestate_fresh_instance_t",

            # these tests also fail with run-tests.py
            "casefolding_t",
            "check_code_t",
            "status_fresh_instance_t",
            "update_unknown_files_t",
        ]
    return excluded

def get_hg_run_tests_excluded():
    return "test_(%s)" % "|".join(excluded_t_tests())

def get_hg_watchman_run_tests_excluded():
    excluded = excluded_t_tests() + excluded_watchman_t_tests()
    return "test_(%s)" % "|".join(excluded)

def get_hg_edenfs_watchman_run_tests_included():
    included = [
        "eden_watchman_edenapi_glob_t",
        "eden_watchman_noedenapi_glob_t",
    ]
    return "test_(%s)" % "|".join(included)

def get_blocklist():
    blocklist_prefix = "$(location //eden/scm/fb/tests:blocklists)/core.tests.blacklist."
    if read_bool("fbcode", "mode_win_enabled", False):
        return blocklist_prefix + "win"
    elif read_bool("fbcode", "mode_mac_enabled", False):
        return blocklist_prefix + "osx"
    return blocklist_prefix + "centos7"

_RT_ENV = {
    "HGEXECUTABLEPATH": "$(location //eden/scm:hg_test)",
    "HGRUNTEST_SKIP_ENV": "1",
    "HGTEST_BLOCKLIST": get_blocklist(),
    "HGTEST_CERTDIR": "$(location //eden/mononoke/tests/integration/certs/facebook:test_certs)",
    # used by unittestify.py
    "HGTEST_DIR": "eden/scm/tests",
    "HGTEST_DUMMYSSH": "$(location :dummyssh3)",
    "HGTEST_EXCLUDED": get_hg_run_tests_excluded(),
    "HGTEST_HG": "$(location //eden/scm:hg_test)",
    "HGTEST_NORMAL_LAYOUT": "0",
    "HGTEST_PYTHON": "fbpython",
    "HGTEST_RUN_TESTS_PY": "$(location :run_tests_py)",
    "HGTEST_SLOWTIMEOUT": "2147483647",
    # used by run-tests.py
    # buck test has its own timeout so just disable run-tests.py
    # timeout practically.
    "HGTEST_TIMEOUT": "2147483647",
    # The one below determines the location of all misc. files required by run-tests.py but not directly
    # imported by it. This is especially important when running in opt mode.
    "RUNTESTDIR": "$(location :test_files)",
    "URLENCODE": "$(location //eden/mononoke/tests/integration:urlencode)",
}

_RT_RESOURCES = {
    "//eden/scm/tests:dummyssh3": "dummyssh3.par",
    "//eden/scm:hg_test": "hg.sh",
    "//eden/scm:hgpython_test": "hgpython.sh",
}

SRCS = dict(
    [("unittestify.py", "unittestify.py")],
)

# Generartes a test target
# Do not use excluded and included at the same time
def run_tests_target(
        name = None,
        watchman = False,
        eden = False,
        mononoke = False,
        env_overrides = dict(),
        excluded = None,
        included = None,
        **kwargs):
    if not name:
        extras = ""
        if eden:
            extras += "edenfs_"
        if watchman:
            extras += "watchman_"
        if mononoke:
            extras += "mononoke_"
        name = "hg_%srun_tests" % extras
    resources = dict(_RT_RESOURCES)
    if not eden:
        ENV = dict(_RT_ENV)
    else:
        artifacts = get_integration_test_env_and_deps()
        ENV = artifacts["env"]
        ENV.update(_RT_ENV)
        ENV["HGTEST_RUN_TESTS_PY"] = "$(location :run_tests_py_eden)"
        ENV["HGTEST_USE_EDEN"] = "1"
    if watchman:
        ENV["HGTEST_WATCHMAN"] = "$(location //watchman:watchman)"
        resources["//watchman:watchman"] = "watchman"
    if mononoke:
        ENV["USE_MONONOKE"] = "1"
        ENV["HGTEST_MONONOKE_SERVER"] = "$(location //eden/mononoke:mononoke)"
        ENV["HGTEST_GET_FREE_SOCKET"] = "$(location //eden/mononoke/tests/integration:get_free_socket)"
        ENV["TEST_FIXTURES"] = "$(location //eden/mononoke/tests/integration:test_fixtures)"
        ENV["JUST_KNOBS_DEFAULTS"] = "$(location //eden/mononoke/mononoke_macros:just_knobs_defaults)"
        ENV["FB_TEST_FIXTURES"] = "$(location //eden/mononoke/tests/integration/facebook:facebook_test_fixtures)"
        resources["//eden/mononoke/tests/integration/certs/facebook:test_certs"] = "certs"
        resources["//eden/mononoke/tests/integration:get_free_socket"] = "get_free_socket.par"
        resources["//eden/mononoke:mononoke"] = "mononoke"
    if excluded:
        ENV["HGTEST_EXCLUDED"] = excluded
    if included:
        ENV["HGTEST_INCLUDED"] = included
        ENV.pop("HGTEST_EXCLUDED")
    for k, v in env_overrides.items():
        if v:
            ENV[k] = v
        else:
            ENV.pop(k)
    python_unittest(
        name = name,
        srcs = SRCS,
        # non-python deps should be in cpp_deps (even if not cpp)
        cpp_deps = [
            "//eden/scm:scm_prompt",
        ],
        env = ENV,
        resources = resources,
        supports_static_listing = False,
        **kwargs
    )
    buck_command_alias(
        name = name + "_cli",
        env = ENV,
        exe = ":unittestify",
        compatible_with = kwargs.pop("compatible_with", None),
    )

def generate_trinity_smoketests(included, **kwargs):
    hg_d = [
        {},
        {
            # Make sure to keep these in sync with unittestify
            "HGEXECUTABLEPATH": None,
            "HGTEST_DEBUGRUNTEST_HG": _RT_ENV["HGEXECUTABLEPATH"],
            "HGTEST_HG": None,
            "HG_REAL_BIN": None,
        },
    ]
    hg_s = ["", "prod_hg_"]
    eden_d = [
        {},
        {
            "EDENFSCTL_REAL_PATH": None,
            "EDENFSCTL_RUST_PATH": None,
            "EDENFS_SERVER_PATH": None,
        },
    ]
    eden_s = ["", "prod_eden_"]
    mononoke_d = [
        {},
        {
            # mononoke_prod is the entire squashfs fbpkg, so we need to get
            # the binary
            "HGTEST_MONONOKE_SERVER": "$(location :mononoke_prod)/mononoke",
        },
    ]
    mononoke_s = ["", "prod_mononoke_"]
    for hg in range(2):
        for eden in range(2):
            for mononoke in range(2):
                if hg + eden + mononoke == 0:
                    # This is the default smoke test, so we don't generate one
                    continue
                name = "trinity_smoke_%stest" % (hg_s[hg] + eden_s[eden] + mononoke_s[mononoke])
                run_tests_target(
                    name = name,
                    eden = True,
                    mononoke = True,
                    watchman = True,
                    env_overrides = hg_d[hg] | eden_d[eden] | mononoke_d[mononoke],
                    included = included,
                    **kwargs
                )
