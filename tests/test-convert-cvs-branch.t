#require cvs

This is https://bz.mercurial-scm.org/1148
and https://bz.mercurial-scm.org/1447

  $ cvscall()
  > {
  >     cvs -f "$@" > /dev/null
  > }
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > convert =
  > [convert]
  > cvsps.cache = 0
  > EOF

create cvs repository

  $ mkdir cvsrepo
  $ cd cvsrepo
  $ CVSROOT=`pwd`
  $ export CVSROOT
  $ CVS_OPTIONS=-f
  $ export CVS_OPTIONS
  $ cd ..
  $ rmdir cvsrepo
  $ cvscall -q -d "$CVSROOT" init

Create a new project

  $ mkdir src
  $ cd src
  $ echo "1" > a
  $ echo "1" > b
  $ cvscall import -m "init" src v0 r0 | sort
  $ cd ..
  $ cvscall co src
  cvs checkout: Updating src
  $ cd src

Branch the project

  $ cvscall tag -b BRANCH
  cvs tag: Tagging .
  $ cvscall up -r BRANCH > /dev/null
  cvs update: Updating .

Modify file a, then b, then a

  $ sleep 1
  $ echo "2" > a
  $ cvscall ci -m "mod a"
  cvs commit: Examining .
  $ echo "2" > b
  $ cvscall ci -m "mod b"
  cvs commit: Examining .
  $ sleep 1
  $ echo "3" > a
  $ cvscall ci -m "mod a again"
  cvs commit: Examining .

Convert

  $ cd ..
  $ hg convert src
  assuming destination src-hg
  initializing destination src-hg repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  7 log entries
  creating changesets
  5 changeset entries
  sorting...
  converting...
  4 Initial revision
  3 init
  2 mod a
  1 mod b
  0 mod a again
  updating tags

Check the result

  $ hg -R src-hg log -G --template '{rev} ({branches}) {desc} files: {files}\n'
  o  5 () update tags files: .hgtags
  |
  | o  4 (BRANCH) mod a again files: a
  | |
  | o  3 (BRANCH) mod b files: b
  | |
  | o  2 (BRANCH) mod a files: a
  | |
  | o  1 (v0) init files:
  |/
  o  0 () Initial revision files: a b
  


issue 1447

  $ cvscall()
  > {
  >     cvs -f "$@" > /dev/null
  >     sleep 1
  > }
  $ cvsci()
  > {
  >     cvs -f ci "$@" >/dev/null
  >     sleep 1
  > }
  $ cvscall -Q -d `pwd`/cvsmaster2 init
  $ cd cvsmaster2
  $ CVSROOT=`pwd`
  $ export CVSROOT
  $ mkdir foo
  $ cd ..
  $ cvscall -Q co -d cvswork2 foo
  $ cd cvswork2
  $ echo foo > a.txt
  $ echo bar > b.txt
  $ cvscall -Q add a.txt b.txt
  $ cvsci -m "Initial commit"
  cvs commit: Examining .
  $ echo foo > b.txt
  $ cvsci -m "Fix b on HEAD"
  cvs commit: Examining .
  $ echo bar > a.txt
  $ cvsci -m "Small fix in a on HEAD"
  cvs commit: Examining .
  $ cvscall -Q tag -b BRANCH
  $ cvscall -Q up -P -rBRANCH
  $ echo baz > b.txt
  $ cvsci -m "Change on BRANCH in b"
  cvs commit: Examining .
  $ hg debugcvsps -x --parents foo
  collecting CVS rlog
  5 log entries
  creating changesets
  4 changeset entries
  ---------------------
  PatchSet 1 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Log:
  Initial commit
  
  Members: 
  	a.txt:INITIAL->1.1 
  	b.txt:INITIAL->1.1 
  
  ---------------------
  PatchSet 2 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Branchpoints: BRANCH 
  Parent: 1
  Log:
  Fix b on HEAD
  
  Members: 
  	b.txt:1.1->1.2 
  
  ---------------------
  PatchSet 3 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Branchpoints: BRANCH 
  Parent: 2
  Log:
  Small fix in a on HEAD
  
  Members: 
  	a.txt:1.1->1.2 
  
  ---------------------
  PatchSet 4 
  Date: * (glob)
  Author: * (glob)
  Branch: BRANCH
  Tag: (none) 
  Parent: 3
  Log:
  Change on BRANCH in b
  
  Members: 
  	b.txt:1.2->1.2.2.1 
  

  $ cd ..
