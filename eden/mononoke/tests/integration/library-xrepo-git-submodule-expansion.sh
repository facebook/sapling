#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"
. "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"

GIT_REPO_A="${TESTTMP}/git-repo-a"
GIT_REPO_B="${TESTTMP}/git-repo-b"
GIT_REPO_C="${TESTTMP}/git-repo-c"
REPO_C_ID=2
REPO_B_ID=3

# Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
export XDG_CONFIG_HOME=$TESTTMP
git config --global protocol.file.allow always


function setup_git_repos_a_b_c {

  print_section "Setting up git repo C to be used as submodule in git repo B"
  mkdir "$GIT_REPO_C"
  cd "$GIT_REPO_C" || exit
  git init -q
  echo "choo" > choo
  git add choo
  git commit -q -am "Add choo"
  mkdir hoo
  cd hoo || exit
  echo "qux" > qux
  cd ..
  git add hoo/qux
  git commit -q -am "Add hoo/qux"
  git log --oneline


  print_section "Setting up git repo B to be used as submodule in git repo A"
  mkdir "$GIT_REPO_B"
  cd "$GIT_REPO_B" || exit
  git init -q
  echo "foo" > foo
  git add foo
  git commit -q -am "Add foo"
  mkdir bar
  cd bar || exit
  echo "zoo" > zoo
  cd ..
  git add bar/zoo
  git commit -q -am "Add bar/zoo"
  git submodule add ../git-repo-c

  git add .
  git commit -q -am "Added git repo C as submodule in B"
  git log --oneline

  tree -a -I ".git"



  print_section "Setting up git repo A"
  mkdir "$GIT_REPO_A"
  cd "$GIT_REPO_A" || exit
  git init -q
  echo "root_file" > root_file
  mkdir duplicates
  echo "Same content" > duplicates/x
  echo "Same content" > duplicates/y
  echo "Same content" > duplicates/z
  git add .
  git commit -q -am "Add root_file"
  mkdir regular_dir
  cd regular_dir || exit
  echo "aardvar" > aardvar
  cd ..
  git add regular_dir/aardvar
  git commit -q -am "Add regular_dir/aardvar"
  git submodule add ../git-repo-b

  git add .
  git commit -q -am "Added git repo B as submodule in A"
  git log --oneline

  git submodule add ../git-repo-c repo_c

  git add . && git commit -q -am "Added git repo C as submodule directly in A"

  tree -a -I ".git"


  cd "$TESTTMP" || exit

}


function gitimport_repos_a_b_c {
  export GIT_REPO_A_HEAD;
  print_section "Importing repos in reverse dependency order, C, B then A"

  REPOID="$REPO_C_ID" quiet gitimport "$GIT_REPO_C" --bypass-derived-data-backfilling \
    --bypass-readonly --generate-bookmarks full-repo

  REPOID="$REPO_B_ID" quiet gitimport "$GIT_REPO_B" --bypass-derived-data-backfilling \
    --bypass-readonly --generate-bookmarks full-repo

  # shellcheck disable=SC2153
  REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling \
    --bypass-readonly --generate-bookmarks full-repo > "$TESTTMP/gitimport_output"

  GIT_REPO_A_HEAD=$(rg ".*Ref: \"refs/heads/master\": Some\(ChangesetId\(Blake2\((\w+).+" -or '$1' "$TESTTMP/gitimport_output")

}

function merge_repo_a_to_large_repo {

  print_section "Importing repo A commits into large repo"
  # shellcheck disable=SC2153
  with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID" "$LARGE_REPO_ID" initial-import \
    --no-progress-bar -i "$GIT_REPO_A_HEAD" \
    --version-name "$LATEST_CONFIG_VERSION_NAME" 2>&1 | tee "$TESTTMP/initial_import_output"

  print_section "Large repo bookmarks"
  mononoke_newadmin bookmarks -R "$LARGE_REPO_NAME" list -S hg


  SYNCED_HEAD=$(rg ".+synced as (\w+) in.+" -or '$1' "$TESTTMP/initial_import_output")
  PARENT=$(mononoke_newadmin fetch -R "$LARGE_REPO_NAME"  -i "$SYNCED_HEAD" --json | jq -r .parents[0])

  print_section "Creating gradual merge commit"
  COMMIT_DATE="1985-09-04T00:00:00.00Z"
  with_stripped_logs megarepo_tool gradual-merge test_user "gradual merge" \
    --pre-deletion-commit "$PARENT" --last-deletion-commit "$SYNCED_HEAD" \
    --bookmark master --limit 1 --commit-date-rfc3339 "$COMMIT_DATE"


  printf "\nSYNCHED_HEAD: %s\n\n" "$SYNCED_HEAD"

  clone_and_log_large_repo "$SYNCED_HEAD"

  hg co -q master

  echo "Large repo tree:"
  tree -a -I ".hg"| tee "${TESTTMP}/large_repo_tree_1"

}

# This will make some changes to all repos, so we can assert that all of them
# are expanded properly and that the submodule pointer update diffs only
# contain the necessary delta (e.g. instead of the entire working copy of
# the new commit).
function make_changes_to_git_repos_a_b_c {
  # These will store the hash of the HEAD commit in each repo after the changes
  export GIT_REPO_A_HEAD;
  export GIT_REPO_B_HEAD;
  export GIT_REPO_C_HEAD;

  print_section "Make changes to repo C"
  cd "$GIT_REPO_C" || exit
  echo 'another file' > choo3 && git add .
  git commit -q -am "commit #3 in repo C"
  echo 'another file' > choo4 && git add .
  git commit -q -am "commit #4 in repo C"
  git log --oneline

  GIT_REPO_C_HEAD=$(git rev-parse HEAD)

  print_section "Update those changes in repo B"
  cd "$GIT_REPO_B" || exit
  git submodule update --remote

  git add .
  git commit -q -am "Update submodule C in repo B"
  rm bar/zoo foo
  git add . && git commit -q -am "Delete files in repo B"
  git log --oneline

  GIT_REPO_B_HEAD=$(git rev-parse HEAD)

  print_section "Update those changes in repo A"
  cd "$GIT_REPO_A" || exit
  # Make simple change directly in repo A
  echo "in A" >> root_file && git add .
  git commit -q -am "Change directly in A"

  print_section "Update submodule b in A"
  git submodule update --remote

  git commit -q -am "Update submodule B in repo A"

  print_section "Then delete repo C submodule used directly in repo A"
  git submodule deinit --force repo_c

  git rm -r repo_c

  git add . && git commit -q -am "Remove repo C submodule from repo A"
  git log --oneline

  GIT_REPO_A_HEAD=$(git rev-parse HEAD)

}


function print_section() {
    printf "\n\nNOTE: %s\n" "$1"
}
