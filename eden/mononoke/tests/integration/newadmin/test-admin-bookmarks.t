# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"

Enable logging of bookmark updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY

setup configuration
  $ setup_common_config "blob_files"
  $ mononoke_testtool drawdag -R repo --derive-all <<'EOF'
  > A-B-C
  >    \
  >     D
  > # bookmark: C main
  > # extra: A convert_revision "svn:22222222-aaaa-0000-aaaa-ddddddddcccc/repo/trunk/project@2077"
  > EOF
  A=7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4
  B=3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371
  C=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777
  D=f448ac19bb6ef701966225d5fb556bd6454673e567e5459b367490e8892008b7

  $ mononoke_admin bookmarks -R repo list -S bonsai,hg
  bonsai=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777 hg=68db22b4319d80682d20d5418cea7be446312b5c main

  $ mononoke_admin bookmarks -R repo get main -S bonsai,hg
  bonsai: aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777
  hg: 68db22b4319d80682d20d5418cea7be446312b5c

  $ mononoke_admin bookmarks -R repo log main -S bonsai,hg
  1 (main) bonsai=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777 hg=68db22b4319d80682d20d5418cea7be446312b5c testmove * (glob)

  $ mononoke_admin bookmarks -R repo set other 3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371
  Creating publishing bookmark other at 3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371

  $ mononoke_admin bookmarks -R repo set other bonsai=7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4 --old-commit-id bonsai=3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371
  Updating publishing bookmark other from 3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371 to 7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4

  $ mononoke_admin bookmarks -R repo get other -S bonsai,hg
  bonsai: 7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4
  hg: 06cc1e6d132edcab226ad7f30976254dc0ce7025

  $ mononoke_admin bookmarks -R repo list -S bonsai,hg
  bonsai=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777 hg=68db22b4319d80682d20d5418cea7be446312b5c main
  bonsai=7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4 hg=06cc1e6d132edcab226ad7f30976254dc0ce7025 other

  $ mononoke_admin bookmarks -R repo log other -S bonsai,hg
  3 (other) bonsai=7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4 hg=06cc1e6d132edcab226ad7f30976254dc0ce7025 manualmove * (glob)
  2 (other) bonsai=3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371 hg=830c7cb9e8f13a11a9426f164edb3c882b40921f manualmove * (glob)

  $ mononoke_admin bookmarks -R repo delete other
  Deleting publishing bookmark other at 7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4

  $ mononoke_admin bookmarks -R repo list -S bonsai,hg
  bonsai=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777 hg=68db22b4319d80682d20d5418cea7be446312b5c main

  $ mononoke_admin bookmarks -R repo get other -S bonsai,hg
  (not set)

Validate that the bookmark updates are logged to scribe
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | sort | jq '{repo_name,bookmark_name,old_bookmark_value,new_bookmark_value,operation}'
  {
    "repo_name": "repo",
    "bookmark_name": "other",
    "old_bookmark_value": null,
    "new_bookmark_value": "3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371",
    "operation": "create"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "other",
    "old_bookmark_value": "3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371",
    "new_bookmark_value": "7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4",
    "operation": "update"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "other",
    "old_bookmark_value": "7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4",
    "new_bookmark_value": null,
    "operation": "delete"
  }
