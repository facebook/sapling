# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["unodes", "git_commits", "git_trees", "git_delta_manifests_v2"]' setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["unodes", "git_commits", "git_trees", "git_delta_manifests_v2"]' setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qa -m "Commit"
  $ git show
  commit 15cc4e9575665b507ee372f97b716ff552842136
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Commit
  
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..433eb17
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +this is file1
# Create a correct tag
  $ git tag -a correct_tag -m ""
# Now create an incorrect tag from this correct tag
  $ git show-ref correct_tag
  596f709c975acae56ccd9fd3e6714beeece4005f refs/tags/correct_tag
  $ git cat-file -p 596f709c975acae56ccd9fd3e6714beeece4005f
  object 15cc4e9575665b507ee372f97b716ff552842136
  type commit
  tag correct_tag
  tagger mononoke <mononoke@mononoke> 946684800 +0000
  
# We will make an incorrect tag by stripping the timezone from the tagger, as was seen in prod during T199503972
# First, show why the tag is invalid, and why git tries to prevent us from creating it
  $ git cat-file -p 596f709c975acae56ccd9fd3e6714beeece4005f | head -c 111 | git mktag
  error: tag input does not pass fsck: unterminatedHeader: unterminated header
  fatal: tag on stdin did not pass our strict fsck check
  [128]
# Nevermind: just need to ask nicely
  $ git cat-file -p 596f709c975acae56ccd9fd3e6714beeece4005f | head -c 111 | git hash-object -w --stdin -t tag --literally
  627a05f23e3182ada1071a3bfaa59dbb527ecba9
# Show our malformed tag for info
  $ git cat-file -p 627a05f23e3182ada1071a3bfaa59dbb527ecba9
  object 15cc4e9575665b507ee372f97b716ff552842136
  type commit
  tag correct_tag
  tagger mononoke <mononoke@mononoke> (no-eol)
  $ git cat-file -t 627a05f23e3182ada1071a3bfaa59dbb527ecba9
  tag
# Make a ref that points to this incorrect tag
  $ echo 627a05f23e3182ada1071a3bfaa59dbb527ecba9 > .git/refs/tags/incorrect_tag

# Import it into Mononoke
  $ with_stripped_logs gitimport "$GIT_REPO" --generate-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:15cc4e95 => Bid:ce423062
  Execution error: read_git_refs failed
  
  Caused by:
      0: unable to read git object: 627a05f23e3182ada1071a3bfaa59dbb527ecba9 for ref: refs/tags/incorrect_tag
      1: Failed to parse:
         ```
         object 15cc4e9575665b507ee372f97b716ff552842136
         type commit
         tag correct_tag
         tagger mononoke <mononoke@mononoke>
         ```
         into object of kind Tag
      2: object parsing failed
  Error: Execution failed


