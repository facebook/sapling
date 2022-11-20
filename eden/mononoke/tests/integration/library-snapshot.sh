#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
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
    ENABLE_API_WRITES=true INFINITEPUSH_ALLOW_WRITES=true setup_common_config
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
    export BASE_SNAPSHOT
    # Make an interesting commit and snapshot that tests all types of file changes
    echo a > modified_file
    echo a > missing_file
    echo a > untouched_file
    echo a > deleted_file
    echo a > deleted_file_then_untracked_modify
    mkdir dir
    echo a > dir/file_in_dir
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
    ln -s symlink_target symlink_file
    BASE_SNAPSHOT=$(HGPLAIN=1 hgedenapi snapshot create --labels testing,labels)
}

function assert_on_base_snapshot {
    # Using subshell to set pipefail
    (
    set -e -o pipefail
    [[ "$(hg log -T "{node}" -r .)" = "$BASE_SNAPSHOT_COMMIT" ]] || ( echo wrong parent && hg log -T "{node}" -r . )
    [[ "$(cat untouched_file)" = "a" ]] || echo wrong untouched_file
    [[ "$(cat dir/file_in_dir)" = "a" ]] || echo wrong file_in_dir
    [[ "$(cat modified_file)" = "b" ]] || echo wrong modified_file
    [[ "$(cat added_file)" = "b" ]] || echo wrong added_file
    [[ "$(cat deleted_file_then_untracked_modify)" = "b" ]] || wrong deleted_file_then_untracked_modify
    [[ "$(cat untracked_file)" = "b" ]] || echo wrong untracked_file
    [[ "$(readlink symlink_file)" = "symlink_target" ]] || echo wrong symlink target
    [[ ! -f deleted_file ]] || echo Existing deleted_file
    [[ ! -f added_file_then_missing ]] || echo Existing added_file_then_missing
    [[ ! -f missing_file ]] || echo Existing missing_file
    )
}
