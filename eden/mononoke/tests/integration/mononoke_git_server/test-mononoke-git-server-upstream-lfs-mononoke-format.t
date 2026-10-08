# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that --upstream-lfs-url-format=mononoke-git-lfs makes mononoke_git_service
# fetch LFS objects via {upstream}/{repo}/download_sha256/{sha256} (the Mononoke
# git LFS shape) instead of the Dewey-style {upstream}/{sha256} shape.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-git-lfs.sh"
  $ GIT_LFS_INTERPRET_POINTERS=1 test_repos_for_lfs_with_upstream
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

# Start mononoke_git_service pointing at the Mononoke LFS server with no path
# segment, and ask it to construct URLs in the mononoke-git-lfs shape (i.e.
# {server}/{repo}/download_sha256/{sha256}). The {repo} segment is filled in
# from the git_server's own repo identity ("repo" here).
  $ mononoke_git_service --upstream-lfs-server "$BASE_LFS_URL" --upstream-lfs-url-format mononoke-git-lfs
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ quiet git_client clone "$CLONE_URL"

# Push an LFS file. Configure the git LFS client to upload to the Mononoke
# LFS server's `repo` namespace so the blob lives at the URL
# {BASE_LFS_URL}/repo/download_sha256/{sha256} that the git_server will fetch.
  $ cd $REPONAME
  $ configure_lfs_client_with_mononoke_server
  $ echo "contents of LFS file fetched via mononoke-git-lfs URL format" > large_file
  $ git lfs track large_file
  Tracking "large_file"
  $ git add .gitattributes large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push

# Show the resulting bonsai as JSON.
  $ mononoke_admin fetch -R repo -B heads/master_bookmark --json | jq
  {
    "changeset_id": "db35da0bcc47fc9be928fae7a3ddb5a0a125c32e9209c0f03179c9f4c09d4b90",
    "parents": [
      "e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2"
    ],
    "author": "mononoke <mononoke@mononoke>",
    "author_date": "2000-01-01T00:00:00Z",
    "committer": "mononoke <mononoke@mononoke>",
    "committer_date": "2000-01-01T00:00:00Z",
    "message": "new LFS change\n",
    "hg_extra": {
      "convert_revision": "e237a0c79a613fc9b5d6c1e71823371061df44b4",
      "hg-git-rename-source": "git"
    },
    "file_changes": {
      ".gitattributes": {
        "Change": {
          "inner": {
            "content_id": "9c803b34f20a6e774db43175832c29c0ec5bc08ab09329f63c619cb03a6ebb7b",
            "file_type": "Regular",
            "size": 47,
            "git_lfs": "FullContent"
          },
          "copy_from": null
        }
      },
      "large_file": {
        "Change": {
          "inner": {
            "content_id": "216a635f6449e5ca132cd5d125d608e31970e0975295ecdfa4268fd01c73bcb4",
            "file_type": "Regular",
            "size": 61,
            "git_lfs": {
              "GitLfsPointer": {
                "non_canonical_pointer": null
              }
            }
          },
          "copy_from": null
        }
      }
    },
    "subtree_changes": {}
  }

# Verify the LFS content was fetched via the new URL shape and stored in
# Mononoke's blobstore.
  $ CONTENT_ID=$(mononoke_admin fetch -R repo -B heads/master_bookmark --json | jq -r '.file_changes.large_file.Change.inner.content_id')
  $ mononoke_admin filestore -R repo fetch --content-id "$CONTENT_ID"
  contents of LFS file fetched via mononoke-git-lfs URL format
