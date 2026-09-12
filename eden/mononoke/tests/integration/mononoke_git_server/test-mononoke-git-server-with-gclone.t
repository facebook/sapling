# Copyright (c) Meta Platforms, Inc. and affiliates.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export CLI_USAGE_LOG=0

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

-- Test gclone git --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a -b master
  $ cat gclone_git_a/file_a.txt
  content A

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_b.git" gclone_git_b -b master
  $ cat gclone_git_b/file_b.txt
  content B

-- Test gclone grepo --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo -b master --require-cached-repo-url
  $ cat gclone_repo/a/file_a.txt
  content A
  $ cat gclone_repo/b/file_b.txt
  content B

-- Test gclone git fails with nonexistent branch --

  $ cd "$TESTTMP"
  $ EXPECTED_RC=1 quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" should_fail -b nonexistent-branch
  [1]

-- Test gclone git with --partial-clone=false --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a_nopartial -b master --partial-clone=false
  $ cat gclone_git_a_nopartial/file_a.txt
  content A

-- Test gclone grepo with --jobs --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo_jobs -b master --require-cached-repo-url --jobs=2
  $ cat gclone_repo_jobs/a/file_a.txt
  content A
  $ cat gclone_repo_jobs/b/file_b.txt
  content B

-- Test gclone git default --check-stat (minimal) --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_a.git" gclone_git_a_chkstat_default -b master
  $ git -C gclone_git_a_chkstat_default config --get core.checkStat
  minimal

-- Test gclone git --check-stat=default --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/repo_b.git" gclone_git_b_chkstat_explicit -b master --check-stat=default
  $ git -C gclone_git_b_chkstat_explicit config --get core.checkStat
  default

-- Test gclone grepo default --check-stat (minimal) on all projects --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo_chkstat -b master --require-cached-repo-url
  $ git -C gclone_repo_chkstat/a config --get core.checkStat
  minimal
  $ git -C gclone_repo_chkstat/b config --get core.checkStat
  minimal

-- Test gclone grepo --check-stat=default on all projects --

  $ cd "$TESTTMP"
  $ quiet "$GCLONE" grepo "$MONONOKE_GIT_SERVICE_BASE_URL/manifest.git" gclone_repo_chkstat_def -b master --require-cached-repo-url --check-stat=default
  $ git -C gclone_repo_chkstat_def/a config --get core.checkStat
  default
  $ git -C gclone_repo_chkstat_def/b config --get core.checkStat
  default
