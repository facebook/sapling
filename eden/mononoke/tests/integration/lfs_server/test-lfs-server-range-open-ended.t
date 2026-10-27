# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# A Range header whose last byte position is u64::MAX is a well-formed range
# that exceeds the blob: the LFS server (strict ranges) rejects it with the
# same error as any other range beyond the end of the blob, and stays up.
#
# FIXME: BUG! Today `Range::range_inclusive(5, u64::MAX)` overflows (`end + 1`).
# With overflow checks (dev builds) the request handler panics and the LFS
# server process dies; without them (opt builds) the range wraps around and the
# server answers 206 with an empty body. Either way the strict-range rejection
# is missing. This test pins the current behaviour; the next diff fixes it.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

  $ lfs_uri="$(lfs_server)/lfs1"

  $ printf 'abcdefghijklmnopqrstuvwxyz' | hg debuglfssend "$lfs_uri"
  71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73 26

# A range that ends inside the blob
  $ curltest -s -w '%{http_code}\n' -H "Range: bytes=5-9" "${lfs_uri}/download_sha256/71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73"
  fghij206

# A range that ends beyond the blob is rejected (strict ranges)
  $ curltest -s -w '%{http_code}\n' -H "Range: bytes=5-100" "${lfs_uri}/download_sha256/71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73"
  {"message":"Could not access Filestore for reads: Range exceeds file size","request_id":"*"}500 (glob)

# FIXME: BUG! A range that ends at u64::MAX should be rejected the same way
# (500, "Range exceeds file size"); today the overflow is never reported: with
# overflow checks the handler panics and curl gets an empty reply (000), without
# them the wrapped range is served as 206 with an empty body.
  $ curltest -s -w '%{http_code}\n' -H "Range: bytes=5-18446744073709551615" "${lfs_uri}/download_sha256/71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73" || true
  (000|206) (re)

# FIXME: BUG! The server should still be healthy afterwards (200); with overflow
# checks the panic took the process down (connection refused, 000).
  $ curltest -s -o /dev/null -w '%{http_code}\n' "${lfs_uri}/download_sha256/71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73" || true
  (000|200) (re)
