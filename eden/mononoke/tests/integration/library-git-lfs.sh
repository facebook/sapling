#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh

# Test setup for testing Git LFS import scenarios, with:
#  * two mononoke repos
#    * "repo" - empty repo ready for import
#    * "legacy_lfs" for simulating external lfs server
#  *  local git repos
#    * $TESTTMP/repo-git-server - bare repo
#    * $TESTTMP/repo-git-client - client configured to push to the above one
#                                 with lfs setup for pushing to legacy_lfs
#
#  The git repo has two files pushed to it, one LFS, one non-LFS.
function test_repos_for_lfs_with_upstream {
    setup_common_config
    REPOTYPE="blob_files"
    setup_common_config $REPOTYPE
    cat >> repos/repo/server.toml <<EOF
    [source_control_service]
    permit_writes = true
    permit_service_writes = true
    permit_commits_without_parents = true
    [source_control_service.service_write_restrictions.gitremoteimport]
    permitted_methods = ["create_bookmark", "move_bookmark", "create_changeset", "set_git_mapping_from_changeset", "git_import_operations"]
    permitted_path_prefixes = [""]
    permitted_bookmark_regex = ".*"
EOF

    # In this test we're creating another repo that serves only as secondary LFS server - this
    # way we're showing tha we can deal with the fact that that file contents are uploaded by git
    # to other LFS server and the import will copy them to Mononoke.
    # (at Meta this simulates our legacy dewey-lfs setup)
    REPOID=2 REPONAME=legacy_lfs setup_common_config $REPOTYPE
    cat >> repos/legacy_lfs/server.toml <<EOF
    [source_control_service]
    permit_writes = true
EOF

    SCUBA_LEGACY_LFS="$TESTTMP/scuba_legacy_lfs_server.json"
    # start LFS server
    LFS_LOG_LEGACY="${TESTTMP}/lfs-legacy.log"
    LFS_LOG="${TESTTMP}/lfs.log"
    quiet lfs_server --tls --log "$LFS_LOG_LEGACY" --scuba-dataset "file://$SCUBA_LEGACY_LFS"
    export LEGACY_LFS_URL
    LEGACY_LFS_URL="$BASE_LFS_URL/legacy_lfs"
    if  [ "$LFS_USE_UPSTREAM" == "1" ]; then
        quiet lfs_server  --tls --log "$LFS_LOG"  --git-blob-upload-allowed  --upstream "${LEGACY_LFS_URL}"
    else
        quiet lfs_server  --tls --log "$LFS_LOG"  --git-blob-upload-allowed
    fi
    export MONONOKE_LFS_URL
    MONONOKE_LFS_URL="$BASE_LFS_URL/repo"
}

function configure_lfs_client_with_legacy_server {
    # configure LFS
    quiet git lfs install --local
    git config --local lfs.url "$LEGACY_LFS_URL"
    git config --local http.extraHeader "x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}"
    git config --local http.sslCAInfo "$TEST_CERTDIR/root-ca.crt"
    git config --local http.sslCert "$TEST_CERTDIR/client0.crt"
    git config --local http.sslKey "$TEST_CERTDIR/client0.key"
}

function test_repos_for_git_lfs_import {
    test_repos_for_lfs_with_upstream
    # Start SCS
    if [ "$START_SCS" == "1" ]; then
        start_and_wait_for_scs_server --scuba-dataset "file://$TESTTMP/scuba.json"
        export SCSC_WRITES_ENABLED=true
    fi
    cd "$TESTTMP"|| return 1

    # create a Git repo and one ordinary commit
    export GIT_REPO_SERVER
    export GIT_REPO_CLIENT
    GIT_REPO_SERVER="${TESTTMP}/repo-git-server"
    GIT_REPO_CLIENT="${TESTTMP}/repo-git-client"
    git init -q "$GIT_REPO_SERVER" -b main --bare
    quiet git clone -q "$GIT_REPO_SERVER" "$GIT_REPO_CLIENT"
    cd "$GIT_REPO_CLIENT"|| return 1
    echo "sml fle" > small_file
    git add small_file
    git commit -aqm "add small ile"

    configure_lfs_client_with_legacy_server
    quiet git lfs track large_file

    # commit LFS file
    echo "laaaaaaaaaarge file" > large_file
    git add large_file
    git commit -aqm "add large file"

    # commit LFS file with non-canonical legacy pointer
    cat >> large_file_non_canonical_pointer <<EOF
version https://hawser.github.com/spec/v1
oid sha256:6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
size 20
EOF
    git add large_file_non_canonical_pointer
    git commit -aqm "add large file non canonical pointer"

    # swap that pointer for contents
    quiet git lfs checkout
    quiet git push -q origin main || return 1
    export GIT_REPO_HEAD
    GIT_REPO_HEAD="$(git rev-parse HEAD)"

    cd "$TESTTMP" || return 1
}
