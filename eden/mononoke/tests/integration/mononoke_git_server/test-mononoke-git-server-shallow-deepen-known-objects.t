# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
# Preserve gitlinks; Mercurial-derived types cannot represent them.
  $ DISABLED_DERIVED_DATA="filenodes hgchangesets hg_augmented_manifests" setup_common_config $REPOTYPE
  $ ORIGIN="$TESTTMP/origin"
  $ mkdir "$ORIGIN"
  $ cd "$ORIGIN"
  $ git init -q
  $ printf 'unchanged content\n' > stable
  $ printf 'required only in older history\n' > historical
  $ git add stable historical
  $ git commit -qm initial
  $ for n in $(seq 1 12); do git commit -q --allow-empty -m "unchanged tree $n"; done
  $ git branch same-tree
  $ git rm -q historical
  $ git commit -qm 'remove historical content from tip'
  $ git branch historical-tip
  $ STABLE=$(git rev-parse HEAD:stable)
  $ HISTORICAL=$(git rev-parse same-tree:historical)
  $ git checkout -q --detach same-tree
  $ git update-index --cacheinfo "160000,$HISTORICAL,historical"
  $ git commit -qm 'replace historical blob with gitlink to the same oid'
  $ git branch gitlink-tip
  $ TREE=$(git rev-parse same-tree^{tree})
  $ LEFT=$(printf 'left\n' | git commit-tree "$TREE" -p same-tree)
  $ RIGHT=$(printf 'right\n' | git commit-tree "$TREE" -p same-tree)
  $ MERGE=$(printf 'merge\n' | git commit-tree "$TREE" -p "$LEFT" -p "$RIGHT")
  $ git branch merge-tip "$MERGE"
# Full-repo import records HEAD as a symbolic ref, not a detached commit.
  $ git symbolic-ref HEAD refs/heads/gitlink-tip

  $ cd "$TESTTMP"
  $ quiet gitimport "$ORIGIN" --generate-bookmarks full-repo
  $ mononoke_git_service

# Read only explicit indexes in the small generated repositories. Keep packs so
# new entries cannot be hidden by automatic repacking or unpacking.
  $ pack_objects () {
  >   for idx in "$1"/.git/objects/pack/*.idx; do git show-index < "$idx"; done | awk '{print $2}' | sort -u
  > }
  $ new_pack_objects () {
  >   for idx in "$1"/.git/objects/pack/*.idx; do
  >     if ! grep -Fxq "$idx" "$2"; then git show-index < "$idx"; fi
  >   done | awk '{print $2}' | sort -u
  > }
  $ check_case () {
  >   branch="$1"
  >   native="$TESTTMP/native-$branch"
  >   mono="$TESTTMP/mono-$branch"
  >   quiet git -c gc.auto=0 clone --no-checkout --no-tags --depth=1 --branch "$branch" "file://$ORIGIN" "$native" || return 1
  >   quiet git_client -c gc.auto=0 clone --no-checkout --no-tags --depth=1 --branch "$branch" "$MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git" "$mono" || return 1
  >   for repo in "$native" "$mono"; do
  >     git -C "$repo" config gc.auto 0
  >     git -C "$repo" config maintenance.auto false
  >     git -C "$repo" config fetch.unpackLimit 1
  >     printf '%s\n' "$repo"/.git/objects/pack/*.idx > "$repo.before-packs"
  >     pack_objects "$repo" > "$repo.before-objects"
  >     grep -Fxq "$STABLE" "$repo.before-objects" || return 1
  >     if test "$branch" = historical-tip || test "$branch" = gitlink-tip; then
  >       if grep -Fxq "$HISTORICAL" "$repo.before-objects"; then return 1; fi
  >     fi
  >   done
  >   quiet git -C "$native" fetch --no-tags --deepen=10 origin "$branch" || return 1
  >   quiet git_client -C "$mono" fetch --no-tags --deepen=10 origin "$branch" || return 1
  >   for repo in "$native" "$mono"; do
  >     new_pack_objects "$repo" "$repo.before-packs" > "$repo.new-objects"
  >     test -s "$repo.new-objects" || return 1
  >     if grep -Fxq "$STABLE" "$repo.new-objects"; then return 1; fi
  >     if test "$branch" = historical-tip || test "$branch" = gitlink-tip; then
  >       grep -Fxq "$HISTORICAL" "$repo.new-objects" || return 1
  >     fi
  >     git -C "$repo" remote set-url origin "$TESTTMP/nonexistent-remote"
  >     GIT_NO_LAZY_FETCH=1 git -C "$repo" cat-file -e "$HISTORICAL" || return 1
  >     quiet env GIT_NO_LAZY_FETCH=1 git -C "$repo" fsck --strict || return 1
  >     GIT_NO_LAZY_FETCH=1 git -C "$repo" rev-list --parents HEAD > "$repo.graph" || return 1
  >   done
  >   diff "$native.graph" "$mono.graph" || return 1
  >   diff "$native/.git/shallow" "$mono/.git/shallow" || return 1
  >   echo "$branch: native and Mononoke preserve history without resending the stable blob"
  > }
  $ check_case same-tree
  same-tree: native and Mononoke preserve history without resending the stable blob
  $ check_case historical-tip
  historical-tip: native and Mononoke preserve history without resending the stable blob
  $ check_case gitlink-tip
  gitlink-tip: native and Mononoke preserve history without resending the stable blob
  $ check_case merge-tip
  merge-tip: native and Mononoke preserve history without resending the stable blob
