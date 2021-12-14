#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"

function base_snapshot_repo_setup {
    if [ $# -eq  0 ]; then
        echo "Must provide at least one clone name"
        exit 1
    fi
    INFINITEPUSH_ALLOW_WRITES=true setup_common_config
    cd "$TESTTMP"
    cat >> "$HGRCPATH" <<EOF
[extensions]
snapshot =
commitcloud =
infinitepush =
amend =
EOF

    hginit_treemanifest repo
    cd repo
    mkcommit "base_commit"

    cd "$TESTTMP"
    for clone in "$@"; do
        hgclone_treemanifest ssh://user@dummy/repo "$clone"
    done
    blobimport repo/.hg repo

    # start mononoke
    mononoke
    wait_for_mononoke
}

function base_commit_and_snapshot {
    export BASE_SNAPSHOT_COMMIT
    # Make an interesting commit and snapshot that tests all types of file changes
    echo a > modified_file
    echo a > missing_file
    echo a > untouched_file
    echo a > deleted_file
    echo a > deleted_file_then_untracked_modify
    hg addremove -q
    hg commit -m "Add base files"
    BASE_SNAPSHOT_COMMIT=$(hg log -T "{node}" -r .)
    EDENSCM_LOG=edenapi::client=error hgedenapi cloud upload -q
    # Create snapshot
    echo b > modified_file
    echo b > untracked_file
    echo b > added_file
    hg add added_file
    echo b > added_file_then_missing
    hg add added_file_then_missing
    rm added_file_then_missing
    rm missing_file
    hg rm deleted_file
    hg rm deleted_file_then_untracked_modify
    echo b > deleted_file_then_untracked_modify
    hgedenapi snapshot create
}

BASE_STATUS="\
M modified_file
A added_file
R deleted_file
R deleted_file_then_untracked_modify
! added_file_then_missing
! missing_file
? untracked_file"

function assert_on_base_snapshot {
    # Using subshell to set pipefail
    (
    set -e -o pipefail
    [[ "$(hg log -T "{node}" -r .)" = "$BASE_SNAPSHOT_COMMIT" ]]
    [[ "$(hg st)" = "$BASE_STATUS" ]]
    [[ "$(cat modified_file)" = "b" ]]
    [[ "$(cat added_file)" = "b" ]]
    [[ "$(cat deleted_file_then_untracked_modify)" = "b" ]]
    [[ "$(cat untracked_file)" = "b" ]]
    [[ ! -f deleted_file ]]
    [[ ! -f added_file_then_missing ]]
    [[ ! -f missing_file ]]
    echo snapshot is correct!
    )
}
