# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=10
  $ LARGE_CONTENT=1234567890123456789
  $ hook_test_setup \
  > limit_commit_size <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_json='''{
  >   "commit_size_limit": $BYTE_LIMIT,
  >   "ignore_path_regexes": ["binaries/bin-.*.tgz", ".graphql$"],
  >   "too_many_files_message": "Too many files: \${count} > \${limit}.",
  >   "too_large_message": "Too large: \${size} > \${limit}."
  > }'''
  > CONF
  > )

Test with ignored paths
  $ hg up -q "min(all())"
  $ mkdir -p binaries
  $ echo $LARGE_CONTENT > binaries/bin-0.tgz
  $ mkdir interfaces
  $ echo $LARGE_CONTENT > interfaces/1.graphql
  $ hg commit -Aqm msg
  $ hgmn push -r . --to master_bookmark
  pushing rev 4b2f324c9502 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ hg up -q "min(all())"
  $ echo $LARGE_CONTENT > bin-1.tgz
  $ hg commit -Aqm msg
  $ hgmn push -r . --to master_bookmark
  pushing rev 74cf48a19435 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for 74cf48a1943545fc730f13c2e5855eabcfa99d48: Too large: 20 > 10.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commit_size for 74cf48a1943545fc730f13c2e5855eabcfa99d48: Too large: 20 > 10.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commit_size for 74cf48a1943545fc730f13c2e5855eabcfa99d48: Too large: 20 > 10."
  abort: unexpected EOL, expected netstring digit
  [255]
