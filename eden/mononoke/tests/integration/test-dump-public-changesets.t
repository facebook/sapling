# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

Dump current entries
  $ quiet mononoke_newadmin dump-public-changesets -R repo --out-filename "$TESTTMP/init-dump"
  $ stat -c '%s %N' "$TESTTMP/init-dump"
  200 '$TESTTMP/init-dump'

Add a new commit
  $ hg up -q "min(all())"
  $ echo "foo" > file2
  $ hg commit -qAm foo
  $ hgmn push -r . --to master_bookmark -q

Dump the extra entry only
  $ quiet mononoke_newadmin dump-public-changesets -R repo --out-filename "$TESTTMP/incr-dump" --start-from-file-end "$TESTTMP/init-dump"
  $ stat -c '%s %N' "$TESTTMP/incr-dump"
  79 '$TESTTMP/incr-dump'

Add a new commit
  $ hg up -q "min(all())"
  $ echo "foo" > file3
  $ hg commit -qAm foo2
  $ hgmn push -r . --to master_bookmark -q

Merge commit files, and compare to a straight dump
  $ quiet mononoke_newadmin dump-public-changesets -R repo --out-filename "$TESTTMP/merge-dump" --start-from-file-end "$TESTTMP/incr-dump" --merge-file "$TESTTMP/init-dump" --merge-file "$TESTTMP/incr-dump"
  $ quiet mononoke_newadmin dump-public-changesets -R repo --out-filename "$TESTTMP/full-dump"
  $ cmp "$TESTTMP/merge-dump" "$TESTTMP/full-dump"
  $ stat -c '%s %N' "$TESTTMP/merge-dump" "$TESTTMP/full-dump"
  356 '$TESTTMP/merge-dump'
  356 '$TESTTMP/full-dump'
