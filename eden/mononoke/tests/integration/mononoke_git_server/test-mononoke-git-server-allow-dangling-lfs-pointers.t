# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify the `x-git-allow-dangling-lfs-pointers` pushvar: when set, a `git
# push` whose LFS pointer can't be resolved (no matching SHA256 in the
# Mononoke filestore in internal mode) still succeeds. The pointer text
# itself becomes the file content and the file is stored as
# `GitLfs::FullContent` in the bonsai. Without the pushvar, the same push
# fails.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ GIT_LFS_INTERPRET_POINTERS=1 setup_common_config $REPOTYPE
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_admin derived-data -R repo derive -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_admin git-symref -R repo create --symref-name HEAD --ref-name master_bookmark --ref-type branch
  Symbolic ref HEAD pointing to branch master_bookmark has been added

# Start mononoke_git_service in internal-lfs mode (the default).
  $ mononoke_git_service --internal-lfs
  $ set_mononoke_as_source_of_truth_for_git

# Clone the repo.
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ quiet git_client clone "$CLONE_URL"

# Disable git-lfs on the client so we can craft a raw LFS pointer file by
# hand and `git push` does NOT try to upload its content anywhere first.
  $ cd $REPONAME
  $ quiet git_client lfs uninstall --local --skip-repo
  $ git config lfs.allowincompletepush true
  $ cat > .gitattributes <<'EOF'
  > dangling_file filter=lfs diff=lfs merge=lfs -text
  > EOF
  $ cat > dangling_file <<'EOF'
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:baaaaaaddddd0000000000000000000000000000000000000000000000000000
  > size 1234
  > EOF
  $ git add .gitattributes dangling_file
  $ git commit -aqm "add dangling LFS pointer"

# Push without the pushvar. The git server tries to resolve the pointer from
# the local filestore, finds nothing under sha256:baaaaaad..., and the push
# fails with an HTTP 500 (the LFS-resolution error bubbles up as a server
# error rather than a clean per-ref rejection — pre-receive packfile parsing
# can't easily emit sideband errors).
  $ git_client push 2>&1 | grep -e "HTTP 500" -e "fatal:" | sort -u
  error: RPC failed; HTTP 500 curl 22 The requested URL returned error: 500
  fatal: the remote end hung up unexpectedly

# Push again with `x-git-allow-dangling-lfs-pointers: 1`. The pointer is
# still unresolvable, but the pushvar tells the git server to accept the
# pointer text itself as the file content. The push succeeds.
  $ git_client -c http.extraHeader="x-git-allow-dangling-lfs-pointers: 1" push
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * master_bookmark -> master_bookmark (glob)

# The bonsai stores `dangling_file` as a regular file (`GitLfs::FullContent`),
# NOT a `GitLfsPointer` — because we couldn't resolve the pointer and fell
# back to storing the pointer bytes verbatim.
  $ mononoke_admin fetch -R repo -B heads/master_bookmark --json \
  >   | jq '.file_changes.dangling_file.Change.inner | {file_type, size, git_lfs}'
  {
    "file_type": "Regular",
    "size": 129,
    "git_lfs": "FullContent"
  }

# Fetch the stored content via the bonsai's content_id and verify it's the
# pointer text — not the (non-existent) `baaaaaad...` payload.
  $ CONTENT_ID=$(mononoke_admin fetch -R repo -B heads/master_bookmark --json | jq -r '.file_changes.dangling_file.Change.inner.content_id')
  $ mononoke_admin filestore -R repo fetch --content-id "$CONTENT_ID"
  version https://git-lfs.github.com/spec/v1
  oid sha256:baaaaaaddddd0000000000000000000000000000000000000000000000000000
  size 1234
