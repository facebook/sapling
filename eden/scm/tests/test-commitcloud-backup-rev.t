#debugruntest-compatible

#require no-eden

  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setconfig extensions.commitcloud=
  $ enable remotenames

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
  $ hg bookmark master
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/server shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *.*s (glob) (?)
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
  commitcloud: head '0d0424fa7cf4' hasn't been uploaded yet
  commitcloud: head 'ecd738f5fb6c' hasn't been uploaded yet
  edenapi: queue 4 commits for upload
  edenapi: queue 7 files for upload
  edenapi: uploaded 7 files
  edenapi: queue 4 trees for upload
  edenapi: uploaded 4 trees
  edenapi: uploaded 4 changesets

  $ hg cloud check -r $A1+$A2+$A3+$B1+$B2
  * backed up (glob)
  * backed up (glob)
  * not backed up (glob)
  * backed up (glob)
  * backed up (glob)

Check backing up new top commit
  $ hg cloud backup $A3
  commitcloud: head '78c4e4751ca8' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

  $ hg cloud backup $A2
  commitcloud: nothing to upload

  $ cd ..

Check that backup doesn't interfere with commit cloud

  $ setconfig commitcloud.hostname=testhost
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > education_page = https://someurl.com/wiki/CommitCloud
  > EOF

  $ cd shallow
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'master' repo
  commitcloud: synchronizing 'master' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg up $B2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ B3=$(mkcommit B3)
  $ hg cloud backup $B3
  commitcloud: head '901656c16420' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

  $ hg cloud sync
  commitcloud: synchronizing 'master' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ mkcommit B4
  7b520430ff426d7f4a6c305bef4a90507afe1b32
  $ hg cloud sync
  commitcloud: synchronizing 'master' with 'user/test/default'
  commitcloud: head '7b520430ff42' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)
