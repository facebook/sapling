# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create two repositories
  $ setup_common_config blob_files
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs_repo

# Start a "server" that never responds as the upstream
  $ upstream_port="$(get_free_socket)"
  $ upstream="http://$(mononoke_host):${upstream_port}/"
  $ ncat --exec "/bin/sleep 1" --keep-open --listen "$LOCALIP" "$upstream_port" &
  $ nc_pid="$!"
  $ echo "$nc_pid" >> "$DAEMON_PIDS"

# Start a LFS server
  $ scuba_proxy="$TESTTMP/scuba.json"
  $ log_proxy="$TESTTMP/lfs_proxy.log"
  $ lfs_proxy="$(lfs_server --upstream "$upstream" --log "$log_proxy" --scuba-log-file "$scuba_proxy")/lfs_repo"

# Import a blob
  $ LFS_HELPER="$(realpath "${TESTTMP}/lfs")"

  $ cat > "$LFS_HELPER" <<EOF
  > #!/bin/bash
  > yes A 2>/dev/null | head -c 2KiB
  > EOF
  $ chmod +x "$LFS_HELPER"

  $ cat > spec << EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746
  > size 2048
  > EOF

  $ mononoke_import lfs "$LFS_HELPER" "$(cat spec)" --repo-id 1
  * lfs_upload: importing blob Sha256(ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746) (glob)
  * lfs_upload: imported blob Sha256(ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746) (glob)

# Downloading a present blob should succeed without talking to the upstream
  $ hg --config extensions.lfs= debuglfsreceive ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048 "$lfs_proxy" | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

  $ cat "$log_proxy"
  IN  > POST /lfs_repo/objects/batch -
  OUT < POST /lfs_repo/objects/batch 200 OK
  IN  > GET /lfs_repo/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d -
  OUT < GET /lfs_repo/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d 200 OK

  $ wait_for_json_record_count "$scuba_proxy" 2
  $ jq -S .normal.batch_order < "$scuba_proxy"
  "internal"
  null

  $ cat "$log_proxy" >> "$log_proxy.saved"
  $ cat "$scuba_proxy" >> "$scuba_proxy.saved"
  $ truncate -s 0 "$log_proxy"
  $ truncate -s 0 "$scuba_proxy"

# Downloading a missing blob should however wait (we check that we took ~4 seconds for this)

  $ hg --config extensions.lfs= debuglfsreceive 0000000000000000000000000000000000000000000000000000000000000000 2048 "$lfs_proxy"
  abort: LFS HTTP error: HTTP Error 500: Internal Server Error (action=download)!
  [255]

  $ cat "$log_proxy"
  IN  > POST /lfs_repo/objects/batch -
  OUT < POST /lfs_repo/objects/batch 500 Internal Server Error

  $ wait_for_json_record_count "$scuba_proxy" 1
  $ jq -S .normal.batch_order < "$scuba_proxy"
  "both"

  $ cat "$log_proxy" >> "$log_proxy.saved"
  $ cat "$scuba_proxy" >> "$scuba_proxy.saved"
  $ truncate -s 0 "$log_proxy"
  $ truncate -s 0 "$scuba_proxy"

# Kill nc, otherwise we don't exit properly :/
  $ killandwait "$nc_pid"
