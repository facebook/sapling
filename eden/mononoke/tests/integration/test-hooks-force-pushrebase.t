# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="main"
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'

  $ setup_common_hg_configs
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOF
  > A X
  > EOF

  $ hg bookmark main -r $A
  $ hg bookmark alternate -r $X

blobimport
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

clone
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ enable pushrebase remotenames

make more commits
  $ drawdag <<EOF
  > D F           # C/large = file_too_large
  > | |           # E/large = file_too_large
  > C E    Z      # Y/large = file_too_large
  > |/     |
  > B      Y
  > |      |
  > |      $X
  > $A
  > EOF

fast-forward the bookmark
  $ hg up -q $B
  $ hgmn push -r . --to main --force
  pushing rev 112478962961 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hgmn push -r . --to main --force
  pushing rev 7ff4b7c298ec to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 5e6585e50f1bf5a236028609e131851379bb311a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 5e6585e50f1bf5a236028609e131851379bb311a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 5e6585e50f1bf5a236028609e131851379bb311a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook, the push will now work
  $ hgmn push -r . --to main --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev 7ff4b7c298ec to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main

attempt a non-fast-forward push over a commit that fails the hook
  $ hg up -q $F
  $ hgmn push -r . --to main --force
  pushing rev af09fbbc2f05 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 18c1f749e0296aca8bbb023822506c1eff9bc8a9: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 18c1f749e0296aca8bbb023822506c1eff9bc8a9: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 18c1f749e0296aca8bbb023822506c1eff9bc8a9: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook, and it should work
  $ hgmn push -r . --to main --pushvar ALLOW_LARGE_FILES=true --force
  pushing rev af09fbbc2f05 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hgmn push -r . --to main --force
  pushing rev e3295448b1ef to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 1cb9b9c4b7dd2e82083766050d166fffe209df6a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 1cb9b9c4b7dd2e82083766050d166fffe209df6a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 1cb9b9c4b7dd2e82083766050d166fffe209df6a: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook, and it should work
  $ hgmn push -r . --to main --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev e3295448b1ef to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
