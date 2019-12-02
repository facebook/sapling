  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob:files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hgmn push -q -r .  --to master_bookmark

  $ hg log -T '{node}\n'
  14961831bd3af3a6331fef7e63367d61cb6c9f6b
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90

  $ cd "$TESTTMP/repo-pull"
  $ hgmn pull -q
  $ hgmn up -q 14961831bd3a

Censor the blacklisted blob (file 'b' in commit '14961831bd3af3a6331fef7e63367d61cb6c9f6b')
  $ mononoke_admin redaction add my_task 14961831bd3af3a6331fef7e63367d61cb6c9f6b b
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)

Restart mononoke
  $ kill $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ setup_common_config blob:files
  $ mononoke
  $ wait_for_mononoke

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  @  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

  $ echo "test" > b
  $ hg ci -q -m "up b"

  $ tglogpnr
  @  0269a088f56a draft 'up b'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Should not succeed since the commit modifies a blacklisted file
  $ hgmn push -q -r .  --to master_bookmark
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ tglogpnr
  @  0269a088f56a draft 'up b'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Restart mononoke and disable redaction verification
  $ kill $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ export REDACTION_DISABLED=1
  $ setup_common_config blob:files
  $ mononoke
  $ wait_for_mononoke

  $ cd "$TESTTMP/repo-pull"

  $ tglogpnr
  @  0269a088f56a draft 'up b'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Even is file b is blacklisted, push won't fail because redaction verification is disabled
  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  0269a088f56a public 'up b'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
