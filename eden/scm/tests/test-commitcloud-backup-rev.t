#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setconfig extensions.commitcloud=

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  >   hg log -T"{node}\n" -r .
  > }

  $ setupcommon

  $ hginit server
  $ cd server
  $ setupserver
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF

  $ mkcommit "base" > /dev/null
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/server shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *.*s (glob)
  $ cd shallow
  $ cat << EOF >> .hg/hgrc
  > [extensions]
  > amend=
  > EOF

Test pushing of specific sets of commits
  $ drawdag <<'EOS'
  > B2 # B1/foo=commit b-2\n
  > |
  > B1 # B1/foo=commit b-1\n
  > |
  > | A3 # A3/foo=commit a-3\n
  > | |
  > | A2 # A2/foo=commit a-2\n
  > | |
  > | A1 # A1/foo=commit a-1\n
  > |/
  > .
  > EOS

  $ hg up $B2 -q

Check backing up top stack commit and mid commit
  $ hg cloud check -r $A2+$B2
  * not backed up (glob)
  * not backed up (glob)

  $ hg cloud backup $A1 $A2 $B2
  backing up stack rooted at * (glob)
  remote: pushing 2 commits:
  remote:     *  A1 (glob)
  remote:     *  A2 (glob)
  backing up stack rooted at * (glob)
  remote: pushing 2 commits:
  remote:     *  B1 (glob)
  remote:     *  B2 (glob)
  commitcloud: backed up 4 commits

  $ hg cloud check -r $A1+$A2+$A3+$B1+$B2
  * backed up (glob)
  * backed up (glob)
  * not backed up (glob)
  * backed up (glob)
  * backed up (glob)

Check backing up new top commit
  $ hg cloud backup $A3
  backing up stack rooted at * (glob)
  remote: pushing 3 commits:
  remote:     *  A1 (glob)
  remote:     *  A2 (glob)
  remote:     *  A3 (glob)
  commitcloud: backed up 1 commit

  $ hg cloud backup $A2
  nothing to back up

  $ cd ..

Check that backup doesn't interfere with commit cloud

  $ setconfig commitcloud.hostname=testhost
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > auth_help = visit https://localhost/oauth to generate a registration token
  > education_page = https://someurl.com/wiki/CommitCloud
  > owner_team = The Test Team @ FB
  > EOF

  $ cd shallow
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'master' repo
  commitcloud: synchronizing 'master' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg up $B2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ B3=$(mkcommit B3)
  $ hg cloud backup $B3
  backing up stack rooted at * (glob)
  remote: pushing 3 commits:
  remote:     *  B1 (glob)
  remote:     *  B2 (glob)
  remote:     *  B3 (glob)
  commitcloud: backed up 1 commit

  $ hg cloud sync
  commitcloud: synchronizing 'master' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ mkcommit B4
  7b520430ff426d7f4a6c305bef4a90507afe1b32
  $ hg cloud sync
  commitcloud: synchronizing 'master' with 'user/test/default'
  backing up stack rooted at 458a3fc7650d
  remote: pushing 4 commits:
  remote:     458a3fc7650d  B1
  remote:     ecd738f5fb6c  B2
  remote:     901656c16420  B3
  remote:     7b520430ff42  B4
  commitcloud: commits synchronized
  finished in * (glob)
