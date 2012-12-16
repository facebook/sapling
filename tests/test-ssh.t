

This test tries to exercise the ssh functionality with a dummy script

creating 'remote' repo

  $ hg init remote
  $ cd remote
  $ echo this > foo
  $ echo this > fooO
  $ hg ci -A -m "init" foo fooO
  $ cat <<EOF > .hg/hgrc
  > [server]
  > uncompressed = True
  > 
  > [hooks]
  > changegroup = python "$TESTDIR/printenv.py" changegroup-in-remote 0 ../dummylog
  > EOF
  $ cd ..

repo not found error

  $ hg clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/nonexistent local
  remote: abort: there is no Mercurial repository here (.hg not found)!
  abort: no suitable response from remote hg!
  [255]

non-existent absolute path

  $ hg clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy//`pwd`/nonexistent local
  remote: abort: there is no Mercurial repository here (.hg not found)!
  abort: no suitable response from remote hg!
  [255]

clone remote via stream

  $ hg clone -e "python \"$TESTDIR/dummyssh\"" --uncompressed ssh://user@dummy/remote local-stream
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

  $ hg clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/remote local
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
  $ echo "changegroup = python \"$TESTDIR/printenv.py\" changegroup-in-local 0 ../dummylog" >> .hg/hgrc

empty default pull

  $ hg paths
  default = ssh://user@dummy/remote
  $ hg pull -e "python \"$TESTDIR/dummyssh\""
  pulling from ssh://user@dummy/remote
  searching for changes
  no changes found

local change

  $ echo bleah > foo
  $ hg ci -m "add"

updating rc

  $ echo "default-push = ssh://user@dummy/remote" >> .hg/hgrc
  $ echo "[ui]" >> .hg/hgrc
  $ echo "ssh = python \"$TESTDIR/dummyssh\"" >> .hg/hgrc

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

  $ hg incoming -R ../remote -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/local
  comparing with ssh://user@dummy/local
  searching for changes
  changeset:   1:a28a9d1a809c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add
  

find incoming on the remote side (using absolute path)

  $ hg incoming -R ../remote -e "python \"$TESTDIR/dummyssh\"" "ssh://user@dummy/`pwd`"
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
  $ hg debugpushkey --config ui.ssh="python \"$TESTDIR/dummyssh\"" ssh://user@dummy/remote namespaces
  bookmarks	
  namespaces	
  phases	
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
  [1]
  $ hg debugpushkey --config ui.ssh="python \"$TESTDIR/dummyssh\"" ssh://user@dummy/remote bookmarks
  foo	1160648e36cec0054048a7edc4110c6f84fde594
  $ hg book -f foo
  $ hg push --traceback
  pushing to ssh://user@dummy/remote
  searching for changes
  no changes found
  updating bookmark foo
  [1]
  $ hg book -d foo
  $ hg in -B
  comparing with ssh://user@dummy/remote
  searching for changed bookmarks
     foo                       a28a9d1a809c
  $ hg book -f -r 0 foo
  $ hg pull -B foo
  pulling from ssh://user@dummy/remote
  no changes found
  updating bookmark foo
  importing bookmark foo
  $ hg book -d foo
  $ hg push -B foo
  pushing to ssh://user@dummy/remote
  searching for changes
  no changes found
  deleting remote bookmark foo
  [1]

a bad, evil hook that prints to stdout

  $ cat <<EOF > $TESTTMP/badhook
  > import sys
  > sys.stdout.write("KABOOM\n")
  > EOF

  $ echo '[hooks]' >> ../remote/.hg/hgrc
  $ echo "changegroup.stdout = python $TESTTMP/badhook" >> ../remote/.hg/hgrc
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
  

clone bookmarks

  $ hg -R ../remote bookmark test
  $ hg -R ../remote bookmarks
   * test                      2:6c0482d977a3
  $ hg clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/remote local-bookmarks
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 4 files (+1 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R local-bookmarks bookmarks
     test                      2:6c0482d977a3

passwords in ssh urls are not supported
(we use a glob here because different Python versions give different
results here)

  $ hg push ssh://user:erroneouspwd@dummy/remote
  pushing to ssh://user:*@dummy/remote (glob)
  abort: password in URL not supported!
  [255]

  $ cd ..

hide outer repo
  $ hg init

Test remote paths with spaces (issue2983):

  $ hg init --ssh "python \"$TESTDIR/dummyssh\"" "ssh://user@dummy/a repo"
  $ touch "$TESTTMP/a repo/test"
  $ hg -R 'a repo' commit -A -m "test"
  adding test
  $ hg -R 'a repo' tag tag
  $ hg id --ssh "python \"$TESTDIR/dummyssh\"" "ssh://user@dummy/a repo"
  73649e48688a

Test (non-)escaping of remote paths with spaces when cloning (issue3145):

  $ hg clone --ssh "python \"$TESTDIR/dummyssh\"" "ssh://user@dummy/a repo"
  destination directory: a repo
  abort: destination 'a repo' is not empty
  [255]

Test hg-ssh using a helper script that will restore PYTHONPATH (which might
have been cleared by a hg.exe wrapper) and invoke hg-ssh with the right
parameters:

  $ cat > ssh.sh << EOF
  > userhost="\$1"
  > SSH_ORIGINAL_COMMAND="\$2"
  > export SSH_ORIGINAL_COMMAND
  > PYTHONPATH="$PYTHONPATH"
  > export PYTHONPATH
  > python "$TESTDIR/../contrib/hg-ssh" "$TESTTMP/a repo"
  > EOF

  $ hg id --ssh "sh ssh.sh" "ssh://user@dummy/a repo"
  73649e48688a

  $ hg id --ssh "sh ssh.sh" "ssh://user@dummy/a'repo"
  remote: Illegal repository "$TESTTMP/a'repo" (glob)
  abort: no suitable response from remote hg!
  [255]

  $ hg id --ssh "sh ssh.sh" --remotecmd hacking "ssh://user@dummy/a'repo"
  remote: Illegal command "hacking -R 'a'\''repo' serve --stdio"
  abort: no suitable response from remote hg!
  [255]

  $ SSH_ORIGINAL_COMMAND="'hg' -R 'a'repo' serve --stdio" python "$TESTDIR/../contrib/hg-ssh"
  Illegal command "'hg' -R 'a'repo' serve --stdio": No closing quotation
  [255]

Test hg-ssh in read-only mode:

  $ cat > ssh.sh << EOF
  > userhost="\$1"
  > SSH_ORIGINAL_COMMAND="\$2"
  > export SSH_ORIGINAL_COMMAND
  > PYTHONPATH="$PYTHONPATH"
  > export PYTHONPATH
  > python "$TESTDIR/../contrib/hg-ssh" --read-only "$TESTTMP/remote"
  > EOF

  $ hg clone --ssh "sh ssh.sh" "ssh://user@dummy/$TESTTMP/remote" read-only-local
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 4 files (+1 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd read-only-local
  $ echo "baz" > bar
  $ hg ci -A -m "unpushable commit" bar
  $ hg push --ssh "sh ../ssh.sh"
  pushing to ssh://user@dummy/*/remote (glob)
  searching for changes
  remote: Permission denied
  remote: abort: prechangegroup.hg-ssh hook failed
  remote: Permission denied
  remote: abort: prepushkey.hg-ssh hook failed
  abort: unexpected response: empty string
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
  changegroup-in-remote hook: HG_NODE=a28a9d1a809cab7d4e2fde4bee738a9ede948b60 HG_SOURCE=serve HG_URL=remote:ssh:127.0.0.1
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  changegroup-in-remote hook: HG_NODE=1383141674ec756a6056f6a9097618482fe0f4a6 HG_SOURCE=serve HG_URL=remote:ssh:127.0.0.1
  Got arguments 1:user@dummy 2:hg -R remote serve --stdio
  Got arguments 1:user@dummy 2:hg init 'a repo'
  Got arguments 1:user@dummy 2:hg -R 'a repo' serve --stdio
  Got arguments 1:user@dummy 2:hg -R 'a repo' serve --stdio
  Got arguments 1:user@dummy 2:hg -R 'a repo' serve --stdio
