# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ mononoke_testtool drawdag -R repo --derive-all <<'EOF'
  > A-B-C
  >    \
  >     D
  > # bookmark: C main
  > # extra: A convert_revision "svn:22222222-aaaa-0000-aaaa-ddddddddcccc/repo/trunk/project@2077"
  > EOF
  *] Reloading redacted config from configerator (glob)
  A=7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4
  B=3c4fe767283b1574d1872a6af9975da0d409da671ad9e3e26c06aef687170371
  C=aa6be8217bef1e8a02b7667b71a3ea721e9f41710e0f33f06b6cb77969be7777
  D=f448ac19bb6ef701966225d5fb556bd6454673e567e5459b367490e8892008b7

  $ mononoke_newadmin convert -R repo -f bonsai -t hg 7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4
  *] Reloading redacted config from configerator (glob)
  06cc1e6d132edcab226ad7f30976254dc0ce7025

  $ mononoke_newadmin convert -R repo -f hg -t bonsai 06cc1e6d132edcab226ad7f30976254dc0ce7025
  *] Reloading redacted config from configerator (glob)
  7bf3f69aa62ffa25186bbb6e6869f0cc297f556bce05a3c639b56f1e3f6f0cf4

TODO: add tests for svnrev and globalrev (requires backfilling the mapping)
