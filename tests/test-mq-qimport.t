#require killdaemons

  $ cat > writelines.py <<EOF
  > import sys
  > path = sys.argv[1]
  > args = sys.argv[2:]
  > assert (len(args) % 2) == 0
  > 
  > f = file(path, 'wb')
  > for i in xrange(len(args)/2):
  >    count, s = args[2*i:2*i+2]
  >    count = int(count)
  >    s = s.decode('string_escape')
  >    f.write(s*count)
  > f.close()
  > 
  > EOF
  > cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > [diff]
  > git = 1
  > EOF
  $ hg init repo
  $ cd repo

qimport without file or revision

  $ hg qimport
  abort: no files or revisions specified
  [255]

qimport non-existing-file

  $ hg qimport non-existing-file
  abort: unable to read file non-existing-file
  [255]

qimport null revision

  $ hg qimport -r null
  abort: revision -1 is not mutable
  (see "hg help phases" for details)
  [255]
  $ hg qseries

import email

  $ hg qimport --push -n email - <<EOF
  > From: Username in email <test@example.net>
  > Subject: [PATCH] Message in email
  > Date: Fri, 02 Jan 1970 00:00:00 +0000
  > 
  > Text before patch.
  > 
  > # HG changeset patch
  > # User Username in patch <test@example.net>
  > # Date 0 0
  > # Node ID 1a706973a7d84cb549823634a821d9bdf21c6220
  > # Parent  0000000000000000000000000000000000000000
  > First line of commit message.
  > 
  > More text in commit message.
  > --- confuse the diff detection
  > 
  > diff --git a/x b/x
  > new file mode 100644
  > --- /dev/null
  > +++ b/x
  > @@ -0,0 +1,1 @@
  > +new file
  > Text after patch.
  > 
  > EOF
  adding email to series file
  applying email
  now at: email

hg tip -v

  $ hg tip -v
  changeset:   0:1a706973a7d8
  tag:         email
  tag:         qbase
  tag:         qtip
  tag:         tip
  user:        Username in patch <test@example.net>
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  First line of commit message.
  
  More text in commit message.
  
  
  $ hg qpop
  popping email
  patch queue now empty
  $ hg qdelete email

import URL

  $ echo foo >> foo
  $ hg add foo
  $ hg diff > url.diff
  $ hg revert --no-backup foo
  $ rm foo

Under unix: file:///foobar/blah
Under windows: file:///c:/foobar/blah

  $ patchurl=`pwd | tr '\\\\' /`/url.diff
  $ expr "$patchurl" : "\/" > /dev/null || patchurl="/$patchurl"
  $ hg qimport file://"$patchurl"
  adding url.diff to series file
  $ rm url.diff
  $ hg qun
  url.diff

import patch that already exists

  $ echo foo2 >> foo
  $ hg add foo
  $ hg diff > ../url.diff
  $ hg revert --no-backup foo
  $ rm foo
  $ hg qimport ../url.diff
  abort: patch "url.diff" already exists
  [255]
  $ hg qpush
  applying url.diff
  now at: url.diff
  $ cat foo
  foo
  $ hg qpop
  popping url.diff
  patch queue now empty

qimport -f

  $ hg qimport -f ../url.diff
  adding url.diff to series file
  $ hg qpush
  applying url.diff
  now at: url.diff
  $ cat foo
  foo2
  $ hg qpop
  popping url.diff
  patch queue now empty

build diff with CRLF

  $ python ../writelines.py b 5 'a\n' 5 'a\r\n'
  $ hg ci -Am addb
  adding b
  $ python ../writelines.py b 2 'a\n' 10 'b\n' 2 'a\r\n'
  $ hg diff > b.diff
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

qimport CRLF diff

  $ hg qimport b.diff
  adding b.diff to series file
  $ hg qpush
  applying b.diff
  now at: b.diff

try to import --push

  $ cat > appendfoo.diff <<EOF
  > append foo
  > 
  > diff -r 07f494440405 -r 261500830e46 baz
  > --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/baz	Thu Jan 01 00:00:00 1970 +0000
  > @@ -0,0 +1,1 @@
  > +foo
  > EOF

  $ cat > appendbar.diff <<EOF
  > append bar
  > 
  > diff -r 07f494440405 -r 261500830e46 baz
  > --- a/baz	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/baz	Thu Jan 01 00:00:00 1970 +0000
  > @@ -1,1 +1,2 @@
  >  foo
  > +bar
  > EOF

  $ hg qimport --push appendfoo.diff appendbar.diff
  adding appendfoo.diff to series file
  adding appendbar.diff to series file
  applying appendfoo.diff
  applying appendbar.diff
  now at: appendbar.diff
  $ hg qfin -a
  patch b.diff finalized without changeset message
  $ touch .hg/patches/2.diff
  $ hg qimport -r 'p1(.)::'
  abort: patch "2.diff" already exists
  [255]
  $ hg qapplied
  3.diff
  $ hg qfin -a
  $ rm .hg/patches/2.diff
  $ hg qimport -r 'p1(.)::' -P
  $ hg qpop -a
  popping 3.diff
  popping 2.diff
  patch queue now empty
  $ hg qdel 3.diff
  $ hg qdel -k 2.diff

qimport -e

  $ hg qimport -e 2.diff
  adding 2.diff to series file
  $ hg qdel -k 2.diff

qimport -e --name newname oldexisitingpatch

  $ hg qimport -e --name this-name-is-better 2.diff
  renaming 2.diff to this-name-is-better
  adding this-name-is-better to series file
  $ hg qser
  this-name-is-better
  url.diff

qimport -e --name without --force

  $ cp .hg/patches/this-name-is-better .hg/patches/3.diff
  $ hg qimport -e --name this-name-is-better 3.diff
  abort: patch "this-name-is-better" already exists
  [255]
  $ hg qser
  this-name-is-better
  url.diff

qimport -e --name with --force

  $ hg qimport --force -e --name this-name-is-better 3.diff
  renaming 3.diff to this-name-is-better
  adding this-name-is-better to series file
  $ hg qser
  this-name-is-better
  url.diff

qimport with bad name, should abort before reading file

  $ hg qimport non-existent-file --name .hg
  abort: patch name cannot begin with ".hg"
  [255]

qimport http:// patch with leading slashes in url

set up hgweb

  $ cd ..
  $ hg init served
  $ cd served
  $ echo a > a
  $ hg ci -Am patch
  adding a
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

  $ cd ../repo
  $ hg qimport http://localhost:$HGPORT/raw-rev/0///
  adding 0 to series file

check qimport phase:

  $ hg -q qpush
  now at: 0
  $ hg phase qparent
  1: draft
  $ hg qimport -r qparent
  $ hg phase qbase
  1: draft
  $ hg qfinish qbase
  $ echo '[mq]' >> $HGRCPATH
  $ echo 'secret=true' >> $HGRCPATH
  $ hg qimport -r qparent
  $ hg phase qbase
  1: secret

  $ cd ..

  $ killdaemons.py $DAEMON_PIDS
