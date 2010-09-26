
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert=
  > EOF

Prepare orig repo

  $ hg init orig
  $ cd orig
  $ echo foo > foo
  $ HGUSER='user name' hg ci -qAm 'foo'
  $ cd ..

Explicit --authors

  $ cat > authormap.txt <<EOF
  > user name = Long User Name
  > 
  > # comment
  > this line is ignored
  > EOF
  $ hg convert --authors authormap.txt orig new
  initializing destination new repository
  Ignoring bad line in author map file authormap.txt: this line is ignored
  scanning source...
  sorting...
  converting...
  0 foo
  Writing author map file new/.hg/authormap
  $ cat new/.hg/authormap
  user name=Long User Name
  $ hg -Rnew log
  changeset:   0:d89716e88087
  tag:         tip
  user:        Long User Name
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo
  
  $ rm -rf new

Implicit .hg/authormap

  $ hg init new
  $ mv authormap.txt new/.hg/authormap
  $ hg convert orig new
  Ignoring bad line in author map file new/.hg/authormap: this line is ignored
  scanning source...
  sorting...
  converting...
  0 foo
  $ hg -Rnew log
  changeset:   0:d89716e88087
  tag:         tip
  user:        Long User Name
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo
  
