# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream)
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_uri="$(lfs_server --log "$lfs_log")/lfs1"

# Send some data
  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$lfs_uri"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Make sure we can read it back
  $ hg debuglfsreceive ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048 "$lfs_uri" | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

# Send again
  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$lfs_uri"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Verify that we only uploaded once
  $ cat "$lfs_log"
  IN  > POST /lfs1/objects/batch -
  OUT < POST /lfs1/objects/batch 200 OK
  IN  > PUT /lfs1/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048?server_hostname=* - (glob)
  OUT < PUT /lfs1/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048?server_hostname=* 200 OK (glob)
  IN  > POST /lfs1/objects/batch -
  OUT < POST /lfs1/objects/batch 200 OK
  IN  > GET /lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d?server_hostname=* - (glob)
  OUT < GET /lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d?server_hostname=* 2* (glob)
  IN  > POST /lfs1/objects/batch -
  OUT < POST /lfs1/objects/batch 200 OK

# Try to download without providing the mandatory client info header
  $ sslcurl_noclientinfo_test "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -s
  {"message:"Error: X-Client-Info header not provided or wrong format (expected json)."} (no-eol)

# Download over a variety of encodings

  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -s -o identity
  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -s -o gzip -H "Accept-Encoding: gzip"
  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -s -o zstd -H "Accept-Encoding: zstd"

# Check that the encoding yield different sizes, but the same content.

  $ wc -c identity
  2048 identity
  $ sha256sum < identity
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

  $ wc -c gzip
  43 gzip
  $ gunzip < gzip | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

  $ wc -c zstd
  18 zstd
  $ zstdcat < zstd | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

# Download with a range

  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -sf --range 0-10 -o chunk0
  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -sf --range 11-2047 -o chunk1

  $ wc -c chunk0
  11 chunk0
  $ wc -c chunk1
  2037 chunk1
  $ cat chunk0 chunk1 | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

  $ curltest "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -sf --range 2048-2049 -o chunk2
  [22]

  $ cat > request <<EOF
  > {
  > "operation": "download",
  >  "transfers": ["basic"],
  >  "objects": [
  >      {
  >          "oid": "ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746",
  >          "size": 2048
  >      }
  >  ]
  > }
  > EOF
  $ curltest -s -w "\n%{http_code}" -H "Host: abcd" "${lfs_uri}/objects/batch/" --data-binary "@request"
  {"message":"Host abcd is not allowlisted","request_id":"*"} (glob)
  400 (no-eol)
  $ curltest -s -w "\n%{http_code}" "${lfs_uri}/objects/batch/" --data-binary "@request"
  {"transfer":"basic","objects":[{"oid":"ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746","size":2048,"authenticated":false,"actions":{"download":{"href":"http://$LOCALIP:*/lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d?server_hostname=*"}}}]} (glob)
  200 (no-eol)
