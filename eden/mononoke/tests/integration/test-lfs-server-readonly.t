# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "open": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "draft": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ setup_common_config
  $ enable lfs

# Create a repository with ACL checking enforcement
  $ ENFORCE_LFS_ACL_CHECK=1 ACL_NAME=open REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1

  $ LIVE_CONFIG="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "enforce_acl_check": false,
  >   "tasks_per_content": 1
  > }
  > EOF

# Start an LFS server
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_ROOT="$(lfs_server --log "$LFS_LOG" --readonly --tls --live-config "$(get_configerator_relative_path "${LIVE_CONFIG}")")"
  $ LFS_URI="$LFS_ROOT/repo1"

# Setup constants. These headers are normally provided by proxygen, they store
# an encoded form of the original client identity. In this case, we have
# USER:test and USER:invalid
  $ DOWNLOAD_URL="$LFS_URI/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"
  $ DOWNLOAD_URL_REPO_ENFORCE_ACL="$LFS_URI_REPO_ENFORCE_ACL/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"

# Upload a blob to both repos
  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$LFS_URI"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Make a request with a valid encoded client identity header, but acl
# enforcement disabled at both the global and repo level.
  $ sslcurlas client0 -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL"
  200

# Enable ACL enforcement killswitch
  $ sed -i 's/"enforce_acl_check": false/"enforce_acl_check": true/g' "$LIVE_CONFIG"
  $ sleep 2

  $ sslcurlas client0 -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL"
  200

  $ yes B 2>/dev/null | head -c 2KiB | hg debuglfssend "$LFS_URI"
  abort: HTTP error: HTTP Error 403: Forbidden (oid=a1bcf2c963bec9588aaa30bd33ef07873792e3ec241453b0d21635d1c4bbae84, action=upload)!
  [255]
