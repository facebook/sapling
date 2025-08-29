# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create two repositories

  $ setup_common_config blob_files
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1
  $ REPOID=2 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 LFS_USE_UPSTREAM=1 setup_mononoke_repo_config lfs2

# Start a LFS server (lfs_upstream is an upstream of lfs_proxy)

  $ scuba_proxy="$TESTTMP/scuba_proxy.json"
  $ scuba_upstream="$TESTTMP/scuba_upstream.json"
  $ log_proxy="$TESTTMP/lfs_proxy.log"
  $ log_upstream="$TESTTMP/lfs_upstream.log"

  $ lfs_upstream="$(lfs_server --log "$log_upstream" --scuba-log-file "$scuba_upstream")"
  $ lfs_proxy="$(lfs_server --always-wait-for-upstream --upstream "${lfs_upstream}/lfs1" --log "$log_proxy" --scuba-log-file "$scuba_proxy")"

# Put content in lfs1 and lfs2

  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "${lfs_upstream}/lfs1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

  $ yes B 2>/dev/null | head -c 2KiB | hg debuglfssend "${lfs_upstream}/lfs2"
  a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84 2048

  $ cat "$log_proxy" >> "$log_proxy.saved"
  $ cat "$log_upstream" >> "$log_upstream.saved"
  $ truncate -s 0 "$log_proxy" "$log_upstream"

# Now, have the proxy sync from upstream to internal. Upstream is lfs1, so to
# do this we send an empty write to lfs2 with the blog that is in lfs1, and we
# expect the proxy to get it from upstream.

  $ curltest -sf -XPUT --data-binary "@/dev/null" "${lfs_proxy}/lfs2/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048"
  $ cat "$log_proxy"
  IN  > PUT /lfs2/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048 -
  OUT < PUT /lfs2/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048 200 OK
  $ cat "$log_upstream"
  IN  > POST /lfs1/objects/batch -
  OUT < POST /lfs1/objects/batch 200 OK
  IN  > GET /lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d?server_hostname=* - (glob)
  OUT < GET /lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d?server_hostname=* 2* (glob)
  $ cat "$log_proxy" >> "$log_proxy.saved"
  $ cat "$log_upstream" >> "$log_upstream.saved"
  $ truncate -s 0 "$log_proxy" "$log_upstream"

# Now, have the proxy sync from internal to upstream. Upstream is sitll lfs1,
# so to do this, we send an empty write for the blob in lfs2, and we expect the
# proxy to push it to upstream.

  $ curltest -sf -XPUT --data-binary "@/dev/null" "${lfs_proxy}/lfs2/upload/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84/2048"
  $ cat "$log_proxy"
  IN  > PUT /lfs2/upload/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84/2048 -
  OUT < PUT /lfs2/upload/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84/2048 200 OK
  $ cat "$log_upstream"
  IN  > POST /lfs1/objects/batch -
  OUT < POST /lfs1/objects/batch 200 OK
  IN  > PUT /lfs1/upload/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84/2048?server_hostname=* - (glob)
  OUT < PUT /lfs1/upload/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84/2048?server_hostname=* 200 OK (glob)
  $ cat "$log_proxy" >> "$log_proxy.saved"
  $ cat "$log_upstream" >> "$log_upstream.saved"
  $ truncate -s 0 "$log_proxy" "$log_upstream"

# Finally, check that this mechanism returns an error if the blob we are
# looking for is not available in either backend.

  $ curltest --silent -XPUT --data-binary "@/dev/null" "${lfs_proxy}/lfs2/upload/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/2048" | jq -S .
  {
    "message": "Upstream batch response included an invalid object: ResponseObject { object: RequestObject { oid: Sha256(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa), size: 2048 }, status: Err { error: ObjectError { code: 404, message: \"Object does not exist\" } } }",
    "request_id": "*" (glob)
  }

# At this point, we expect both repos to have both blobs.

  $ curltest -sf -w '%{http_code}\n' -o /dev/null "${lfs_upstream}/lfs1/download_sha256/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746"
  200
  $ curltest -sf -w '%{http_code}\n' -o /dev/null "${lfs_upstream}/lfs2/download_sha256/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746"
  200

  $ curltest -sf -w '%{http_code}\n' -o /dev/null "${lfs_upstream}/lfs1/download_sha256/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84"
  200
  $ curltest -sf -w '%{http_code}\n' -o /dev/null "${lfs_upstream}/lfs2/download_sha256/a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84"
  200
