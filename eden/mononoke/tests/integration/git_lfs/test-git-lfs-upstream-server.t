# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-git-lfs.sh"
  $ GIT_LFS_INTERPRET_POINTERS=1 test_repos_for_lfs_with_upstream
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo derive -T git_trees -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name main --ref-type branch
  Symbolic ref HEAD pointing to branch main has been added

# Start up the Mononoke Git Service
  $ mononoke_git_service --upstream-lfs-server "$LEGACY_LFS_URL/download_sha256"
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ git_client clone "$CLONE_URL"
  Cloning into 'repo'...

# Push with legacy server configures to check if the LFS files end up in mononoke anyway
  $ cd $REPONAME  
  $ configure_lfs_client_with_legacy_server
  $ echo "contents of LFS file that will be uploaded to legacy server" > large_file
  $ git lfs track large_file
  Tracking "large_file"
  $ git add .gitattributes large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push
  $ mononoke_newadmin fetch -R repo -B heads/main
  BonsaiChangesetId: 461f3f262ea85981840edbfa22e991077dcff220624bb0a6b61f834475b87823
  Author: mononoke <mononoke@mononoke>
  Message: new LFS change
  
  FileChanges:
  	 ADDED/MODIFIED: .gitattributes 9c803b34f20a6e774db43175832c29c0ec5bc08ab09329f63c619cb03a6ebb7b
  	 ADDED/MODIFIED (LFS): large_file 978e55f6ff83794e598f13fb0f4f30bca32dd1dda8b57df5983a4dba00cc7ef2
  
  $ mononoke_newadmin filestore -R repo fetch  --content-id 978e55f6ff83794e598f13fb0f4f30bca32dd1dda8b57df5983a4dba00cc7ef2
  contents of LFS file that will be uploaded to legacy server
