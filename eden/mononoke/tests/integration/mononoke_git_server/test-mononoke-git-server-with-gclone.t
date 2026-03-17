# Copyright (c) Meta Platforms, Inc. and affiliates.

  $ . "${TEST_FIXTURES}/library.sh"

-- Setup repos --

  $ REPOID=0 REPONAME=repo_a setup_common_config blob_files
  $ REPOID=1 REPONAME=repo_b setup_common_config blob_files
  $ REPOID=2 REPONAME=manifest setup_common_config blob_files

-- Start git server --

  $ mononoke_git_service

-- Create and import repo_a --

  $ GIT_REPO_A="${TESTTMP}/git_repo_a"
  $ mkdir -p "$GIT_REPO_A" && cd "$GIT_REPO_A"
  $ git init -q -b master
  $ echo "content A" > file_a.txt
  $ git add file_a.txt
  $ git commit -qam "Initial commit for repo_a"
  $ SHA_A=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=0 quiet gitimport "$GIT_REPO_A" --derive-hg --generate-bookmarks full-repo

-- Create and import repo_b --

  $ GIT_REPO_B="${TESTTMP}/git_repo_b"
  $ mkdir -p "$GIT_REPO_B" && cd "$GIT_REPO_B"
  $ git init -q -b master
  $ echo "content B" > file_b.txt
  $ git add file_b.txt
  $ git commit -qam "Initial commit for repo_b"
  $ SHA_B=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=1 quiet gitimport "$GIT_REPO_B" --derive-hg --generate-bookmarks full-repo

-- Create and import manifest repo --

  $ GIT_MANIFEST="${TESTTMP}/git_manifest"
  $ mkdir -p "$GIT_MANIFEST" && cd "$GIT_MANIFEST"
  $ git init -q -b master
  $ cat > default.xml << EOF
  > <?xml version="1.0" encoding="UTF-8"?>
  > <manifest>
  >   <remote name="origin" fetch="$MONONOKE_GIT_SERVICE_BASE_URL"/>
  >   <default remote="origin" revision="master"/>
  >   <project name="repo_a" path="a" revision="$SHA_A"/>
  >   <project name="repo_b" path="b" revision="$SHA_B"/>
  > </manifest>
  > EOF
  $ git add default.xml
  $ git commit -qam "Initial manifest"
  $ cd "$TESTTMP"
  $ REPOID=2 quiet gitimport "$GIT_MANIFEST" --derive-hg --generate-bookmarks full-repo

-- Configure SSL for gclone --

  $ git config --global http.sslCAInfo "$TEST_CERTDIR/root-ca.crt"
  $ git config --global http.sslCert "$TEST_CERTDIR/client0.crt"
  $ git config --global http.sslKey "$TEST_CERTDIR/client0.key"

-- Test gclone git (upload) --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a_upload -b master --upload
  $ cat gclone_git_a_upload/file_a.txt
  content A

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_b.git" gclone_git_b_upload -b master --upload
  $ cat gclone_git_b_upload/file_b.txt
  content B

-- Test gclone git (download) --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a -b master
  $ cat gclone_git_a/file_a.txt
  content A

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_b.git" gclone_git_b -b master
  $ cat gclone_git_b/file_b.txt
  content B

-- Test gclone grepo (upload) --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo_upload -b master --require-cached-repo-url --upload
  $ cat gclone_repo_upload/a/file_a.txt
  content A
  $ cat gclone_repo_upload/b/file_b.txt
  content B

-- Test gclone grepo (download) --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo -b master --require-cached-repo-url
  $ cat gclone_repo/a/file_a.txt
  content A
  $ cat gclone_repo/b/file_b.txt
  content B

-- Test gclone git with --refresh-index-stats --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a_refresh -b master --refresh-index-stats
  $ cat gclone_git_a_refresh/file_a.txt
  content A
  $ cd gclone_git_a_refresh && git status --short
  $ cd "$TESTTMP"

-- Test gclone grepo with --refresh-index-stats --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo_refresh -b master --require-cached-repo-url --refresh-index-stats
  $ cat gclone_repo_refresh/a/file_a.txt
  content A
  $ cat gclone_repo_refresh/b/file_b.txt
  content B
  $ cd gclone_repo_refresh && .repo/repo/repo forall -c 'git status --short'
  $ cd "$TESTTMP"
