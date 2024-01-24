load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")

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

def get_blocklist():
    blocklist_prefix = "../fb/tests/core.tests.blacklist."
    if read_bool("fbcode", "mode_win_enabled", False):
        return blocklist_prefix + "win"
    elif read_bool("fbcode", "mode_mac_enabled", False):
        return blocklist_prefix + "osx"
    return blocklist_prefix + "centos7"
