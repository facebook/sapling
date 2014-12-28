#require cvs

  $ filterpath()
  > {
  >     eval "$@" | sed "s:$CVSROOT:*REPO*:g"
  > }
  $ cvscall()
  > {
  >     cvs -f "$@"
  > }

output of 'cvs ci' varies unpredictably, so discard most of it
-- just keep the part that matters

  $ cvsci()
  > {
  >     cvs -f ci -f "$@" > /dev/null
  > }
  $ hgcat()
  > {
  >     hg --cwd src-hg cat -r tip "$1"
  > }
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH

create cvs repository

  $ mkdir cvsmaster
  $ cd cvsmaster
  $ CVSROOT=`pwd`
  $ export CVSROOT
  $ CVS_OPTIONS=-f
  $ export CVS_OPTIONS
  $ cd ..
  $ rmdir cvsmaster
  $ filterpath cvscall -Q -d "$CVSROOT" init

checkout #1: add foo.txt

  $ cvscall -Q checkout -d cvsworktmp .
  $ cd cvsworktmp
  $ mkdir foo
  $ cvscall -Q add foo
  $ cd foo
  $ echo foo > foo.txt
  $ cvscall -Q add foo.txt
  $ cvsci -m "add foo.txt" foo.txt
  $ cd ../..
  $ rm -rf cvsworktmp

checkout #2: create MYBRANCH1 and modify foo.txt on it

  $ cvscall -Q checkout -d cvswork foo
  $ cd cvswork
  $ cvscall -q rtag -b -R MYBRANCH1 foo
  $ cvscall -Q update -P -r MYBRANCH1
  $ echo bar > foo.txt
  $ cvsci -m "bar" foo.txt
  $ echo baz > foo.txt
  $ cvsci -m "baz" foo.txt

create MYBRANCH1_2 and modify foo.txt some more

  $ cvscall -q rtag -b -R -r MYBRANCH1 MYBRANCH1_2 foo
  $ cvscall -Q update -P -r MYBRANCH1_2
  $ echo bazzie > foo.txt
  $ cvsci -m "bazzie" foo.txt

create MYBRANCH1_1 and modify foo.txt yet again

  $ cvscall -q rtag -b -R MYBRANCH1_1 foo
  $ cvscall -Q update -P -r MYBRANCH1_1
  $ echo quux > foo.txt
  $ cvsci -m "quux" foo.txt

merge MYBRANCH1 to MYBRANCH1_1

  $ filterpath cvscall -Q update -P -jMYBRANCH1
  rcsmerge: warning: conflicts during merge
  RCS file: *REPO*/foo/foo.txt,v
  retrieving revision 1.1
  retrieving revision 1.1.2.2
  Merging differences between 1.1 and 1.1.2.2 into foo.txt

carefully placed sleep to dodge cvs bug (optimization?) where it
sometimes ignores a "commit" command if it comes too fast (the -f
option in cvsci seems to work for all the other commits in this
script)

  $ sleep 1
  $ echo xyzzy > foo.txt
  $ cvsci -m "merge1+clobber" foo.txt

#if unix-permissions

return to trunk and merge MYBRANCH1_2

  $ cvscall -Q update -P -A
  $ filterpath cvscall -Q update -P -jMYBRANCH1_2
  RCS file: *REPO*/foo/foo.txt,v
  retrieving revision 1.1
  retrieving revision 1.1.2.2.2.1
  Merging differences between 1.1 and 1.1.2.2.2.1 into foo.txt
  $ cvsci -m "merge2" foo.txt
  $ REALCVS=`which cvs`
  $ echo "for x in \$*; do if [ \"\$x\" = \"rlog\" ]; then echo \"RCS file: $CVSROOT/foo/foo.txt,v\"; cat \"$TESTDIR/test-convert-cvsnt-mergepoints.rlog\"; exit 0; fi; done; $REALCVS \$*" > ../cvs
  $ chmod +x ../cvs
  $ PATH=..:${PATH} hg debugcvsps --parents foo
  collecting CVS rlog
  7 log entries
  creating changesets
  7 changeset entries
  ---------------------
  PatchSet 1 
  Date: * (glob)
  Author: user
  Branch: HEAD
  Tag: (none) 
  Branchpoints: MYBRANCH1, MYBRANCH1_1 
  Log:
  foo.txt
  
  Members: 
  	foo.txt:INITIAL->1.1 
  
  ---------------------
  PatchSet 2 
  Date: * (glob)
  Author: user
  Branch: MYBRANCH1
  Tag: (none) 
  Parent: 1
  Log:
  bar
  
  Members: 
  	foo.txt:1.1->1.1.2.1 
  
  ---------------------
  PatchSet 3 
  Date: * (glob)
  Author: user
  Branch: MYBRANCH1
  Tag: (none) 
  Branchpoints: MYBRANCH1_2 
  Parent: 2
  Log:
  baz
  
  Members: 
  	foo.txt:1.1.2.1->1.1.2.2 
  
  ---------------------
  PatchSet 4 
  Date: * (glob)
  Author: user
  Branch: MYBRANCH1_1
  Tag: (none) 
  Parent: 1
  Log:
  quux
  
  Members: 
  	foo.txt:1.1->1.1.4.1 
  
  ---------------------
  PatchSet 5 
  Date: * (glob)
  Author: user
  Branch: MYBRANCH1_2
  Tag: (none) 
  Parent: 3
  Log:
  bazzie
  
  Members: 
  	foo.txt:1.1.2.2->1.1.2.2.2.1 
  
  ---------------------
  PatchSet 6 
  Date: * (glob)
  Author: user
  Branch: HEAD
  Tag: (none) 
  Parents: 1,5
  Log:
  merge
  
  Members: 
  	foo.txt:1.1->1.2 
  
  ---------------------
  PatchSet 7 
  Date: * (glob)
  Author: user
  Branch: MYBRANCH1_1
  Tag: (none) 
  Parents: 4,3
  Log:
  merge
  
  Members: 
  	foo.txt:1.1.4.1->1.1.4.2 
  
#endif

  $ cd ..
