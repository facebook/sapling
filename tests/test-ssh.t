

This test tries to exercise the ssh functionality with a dummy script

  $ cat <<EOF > dummyssh
  > import sys
  > import os
  > 
  > os.chdir(os.path.dirname(sys.argv[0]))
  > if sys.argv[1] != "user@dummy":
  >     sys.exit(-1)
  > 
  > if not os.path.exists("dummyssh"):
  >     sys.exit(-1)
  > 
  > os.environ["SSH_CLIENT"] = "127.0.0.1 1 2"
  > 
  > log = open("dummylog", "ab")
  > log.write("Got arguments")
  > for i, arg in enumerate(sys.argv[1:]):
  >     log.write(" %d:%s" % (i+1, arg))
  > log.write("\n")
  > log.close()
  > r = os.system(sys.argv[2])
  > sys.exit(bool(r))
  > EOF
  $ cat <<EOF > badhook
  > import sys
  > sys.stdout.write("KABOOM\n")
  > EOF

creating 'remote

  $ hg init remote
  $ cd remote
  $ echo this > foo
  $ echo this > fooO
  $ hg ci -A -m "init" foo fooO
  $ echo <<EOF > .hg/hgrc
  > [server]
  > uncompressed = True
  > 
  > [hooks]
  > changegroup = python "$TESTDIR"/printenv.py changegroup-in-remote 0 ../dummylog
  > EOF
  $ cd ..

repo not found error

  $ hg clone -e "python ./dummyssh" ssh://user@dummy/nonexistent local
  remote: abort: There is no Mercurial repository here (.hg not found)!
  abort: no suitable response from remote hg!
  [255]

non-existent absolute path

  $ hg clone -e "python ./dummyssh" ssh://user@dummy//`pwd`/nonexistent local
  remote: abort: There is no Mercurial repository here (.hg not found)!
  abort: no suitable response from remote hg!
  [255]

clone remote via stream

  $ hg clone -e "python ./dummyssh" --uncompressed ssh://user@dummy/remote local-stream
  streaming all changes
  4 files to transfer, 392 bytes of data
  transferred 392 bytes in * seconds (*/sec) (glob)
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd local-stream
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 1 changesets, 2 total revisions
  $ cd ..

clone remote via pull

  $ hg clone -e "python ./dummyssh" ssh://user@dummy/remote local
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

verify

  $ cd local
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 1 changesets, 2 total revisions
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'changegroup = python "$TESTDIR"/printenv.py changegroup-in-local 0 ../dummylog' >> .hg/hgrc

empty default pull

  $ hg paths
  default = ssh://user@dummy/remote
  $ hg pull -e "python ../dummyssh"
  pulling from ssh://user@dummy/remote
  searching for changes
  no changes found

local change

  $ echo bleah > foo
  $ hg ci -m "add"

updating rc

  $ echo "default-push = ssh://user@dummy/remote" >> .hg/hgrc
  $ echo "[ui]" >> .hg/hgrc
  $ echo "ssh = python ../dummyssh" >> .hg/hgrc

find outgoing

  $ hg out ssh://user@dummy/remote
  comparing with ssh://user@dummy/remote
  searching for changes
  changeset:   1:a28a9d1a809c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add
  

find incoming on the remote side

  $ hg incoming -R ../remote -e "python ../dummyssh" ssh://user@dummy/local
  comparing with ssh://user@dummy/local
  searching for changes
  changeset:   1:a28a9d1a809c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add
  

find incoming on the remote side (using absolute path)

  $ hg incoming -R ../remote -e "python ../dummyssh" "ssh://user@dummy/`pwd`"
  comparing with ssh://user@dummy/$TESTTMP/local
  searching for changes
  changeset:   1:a28a9d1a809c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add
  

push

  $ hg push
  pushing to ssh://user@dummy/remote
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ cd ../remote

check remote tip

  $ hg tip
  changeset:   1:a28a9d1a809c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add
  
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 3 total revisions
  $ hg cat -r tip foo
  bleah
  $ echo z > z
  $ hg ci -A -m z z
  created new head

test pushkeys and bookmarks

  $ cd ../local
  $ hg debugpushkey --config ui.ssh="python ../dummyssh" ssh://user@dummy/remote namespaces
  bookmarks	
  namespaces	
  $ hg book foo -r 0
  $ hg out -B
  comparing with ssh://user@dummy/remote
  searching for changed bookmarks
     foo                       1160648e36ce
  $ hg push -B foo
  pushing to ssh://user@dummy/remote
  searching for changes
  no changes found
  exporting bookmark foo
  $ hg debugpushkey --config ui.ssh="python ../dummyssh" ssh://user@dummy/remote bookmarks
  foo	1160648e36cec0054048a7edc4110c6f84fde594
  $ hg book -f foo
  $ hg push --traceback
  pushing to ssh://user@dummy/remote
  searching for changes
  no changes found
  updating bookmark foo
  $ hg book -d foo
  $ hg in -B
  comparing with ssh://user@dummy/remote
  searching for changed bookmarks
     foo                       a28a9d1a809c
  $ hg book -f -r 0 foo
  $ hg pull -B foo
  pulling from ssh://user@dummy/remote
  searching for changes
  no changes found
  updating bookmark foo
  importing bookmark foo
  $ hg book -d foo
  $ hg push -B foo
  pushing to ssh://user@dummy/remote
  searching for changes
  no changes found
  deleting remote bookmark foo

a bad, evil hook that prints to stdout

  $ echo '[hooks]' >> ../remote/.hg/hgrc
  $ echo 'changegroup.stdout = python ../badhook' >> ../remote/.hg/hgrc
  $ echo r > r
  $ hg ci -A -m z r

push should succeed even though it has an unexpected response

  $ hg push
  pushing to ssh://user@dummy/remote
  searching for changes
  note: unsynced remote changes!
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: KABOOM
  $ hg -R ../remote heads
  changeset:   3:1383141674ec
  tag:         tip
  parent:      1:a28a9d1a809c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     z
  
  changeset:   2:6c0482d977a3
  parent:      0:1160648e36ce
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     z
  

passwords in ssh urls are not supported

  $ hg push ssh://user:erroneouspwd@dummy/remote
  abort: password in URL not supported!
  [255]

  $ cd ..
  $ cat dummylog
  Got arguments 1:user@dummy 2:hg -R nonexistent serve --stdio
  Got arguments 1:user@dummy 2:hg -R /$TESTTMP/nonexistent serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R local serve --stdio
  Got arguments 1:user@dummy 2:hg -R $TESTTMP/local serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
