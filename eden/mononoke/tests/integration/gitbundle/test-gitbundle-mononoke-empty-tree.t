# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

Setup repository starting with empty tree and ending with empty tree
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C
  > # bookmark: A heads/main
  > # modify: B plain_file something
  > # delete: C plain_file
  > EOF
  A=1b67a29aa79a804c85c94f1bef677daddf199deca00394423b2e8efef5efe6a8
  B=07b7edeeab956a1151c20254f1b09add84640afe8a4ca58d5134cbd20a459db5
  C=d670a93c7d77e055ce95f568fcf4cbf6176af1b290762c61aa76ee3f34c74ed0

Generate Git repo out of the Mononoke repo
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name main --ref-type branch
  Symbolic ref HEAD pointing to branch main has been added
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
  Error: Error in writing packfile items to bundle
  
  Caused by:
      0: Failure in fetching Packfile Item from stream
      1: Error in fetching raw git object bytes for object Sha1(4b825dc642cb6eb9a060e54bf8d69288fbee4904) while generating packfile item
      2: The object corresponding to object ID 4b825dc642cb6eb9a060e54bf8d69288fbee4904 or its packfile item does not exist in the data store
  [1]
Test bundled repo verification
  $ git init -q empty_repo
  $ cd empty_repo
  $ git bundle verify -q $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay
  $ cd ..
Test cloning the bundled repo
  $ git clone $BUNDLE_PATH cloned_git_repo
  Cloning into 'cloned_git_repo'...
  fatal: early EOF
  error: index-pack died
  fatal: remote transport reported error
  [128]

Test batched derivation
  $ mononoke_newadmin derived-data -R "repo" derive --all-types -i "$C"
  Error: failed to derive git_delta_manifests batch (start:07b7edeeab956a1151c20254f1b09add84640afe8a4ca58d5134cbd20a459db5, end:07b7edeeab956a1151c20254f1b09add84640afe8a4ca58d5134cbd20a459db5)
  
  Caused by:
      0: Error in generating git delta manifest entry for path 
      1: The object corresponding to object ID 4b825dc642cb6eb9a060e54bf8d69288fbee4904 or its packfile item does not exist in the data store
  [1]
