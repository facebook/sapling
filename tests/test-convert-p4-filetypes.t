#require p4 execbit symlink

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH

create p4 depot
  $ P4ROOT=`pwd`/depot; export P4ROOT
  $ P4AUDIT=$P4ROOT/audit; export P4AUDIT
  $ P4JOURNAL=$P4ROOT/journal; export P4JOURNAL
  $ P4LOG=$P4ROOT/log; export P4LOG
  $ P4PORT=localhost:$HGPORT; export P4PORT
  $ P4DEBUG=1; export P4DEBUG
  $ P4CHARSET=utf8; export P4CHARSET

start the p4 server
  $ [ ! -d $P4ROOT ] && mkdir $P4ROOT
  $ p4d -f -J off -xi >$P4ROOT/stdout 2>$P4ROOT/stderr
  $ p4d -f -J off >$P4ROOT/stdout 2>$P4ROOT/stderr &
  $ echo $! >> $DAEMON_PIDS
  $ trap "echo stopping the p4 server ; p4 admin stop" EXIT

wait for the server to initialize
  $ while ! p4 ; do
  >    sleep 1
  > done >/dev/null 2>/dev/null

create a client spec
  $ P4CLIENT=hg-p4-import; export P4CLIENT
  $ DEPOTPATH=//depot/test-mercurial-import/...
  $ p4 client -o | sed '/^View:/,$ d' >p4client
  $ echo View: >>p4client
  $ echo " $DEPOTPATH //$P4CLIENT/..." >>p4client
  $ p4 client -i <p4client
  Client hg-p4-import saved.

populate the depot
  $ TYPES="text binary symlink"
  $ TYPES="$TYPES text+m text+w text+x text+k text+kx text+ko text+l text+C text+D text+F text+S text+S2"
  $ TYPES="$TYPES binary+k binary+x binary+kx symlink+k"
  $ TYPES="$TYPES ctext cxtext ktext kxtext ltext tempobj ubinary uxbinary xbinary xltext xtempobj xtext"
not testing these
  $ #TYPES="$TYPES apple resource unicode utf16 uresource xunicode xutf16"
  $ for T in $TYPES ; do
  >    T2=`echo $T | tr [:upper:] [:lower:]`
  >    case $T in
  >       apple)
  >          ;;
  >       symlink*)
  >          echo "this is target $T" >target_$T2
  >          ln -s target_$T file_$T2
  >          p4 add target_$T2
  >          p4 add -t $T file_$T2
  >          ;;
  >       binary*)
  >          $PYTHON -c "file('file_$T2', 'wb').write('this is $T')"
  >          p4 add -t $T file_$T2
  >          ;;
  >       *)
  >          echo "this is $T" >file_$T2
  >          p4 add -t $T file_$T2
  >          ;;
  >    esac
  > done
  //depot/test-mercurial-import/file_text#1 - opened for add
  //depot/test-mercurial-import/file_binary#1 - opened for add
  //depot/test-mercurial-import/target_symlink#1 - opened for add
  //depot/test-mercurial-import/file_symlink#1 - opened for add
  //depot/test-mercurial-import/file_text+m#1 - opened for add
  //depot/test-mercurial-import/file_text+w#1 - opened for add
  //depot/test-mercurial-import/file_text+x#1 - opened for add
  //depot/test-mercurial-import/file_text+k#1 - opened for add
  //depot/test-mercurial-import/file_text+kx#1 - opened for add
  //depot/test-mercurial-import/file_text+ko#1 - opened for add
  //depot/test-mercurial-import/file_text+l#1 - opened for add
  //depot/test-mercurial-import/file_text+c#1 - opened for add
  //depot/test-mercurial-import/file_text+d#1 - opened for add
  //depot/test-mercurial-import/file_text+f#1 - opened for add
  //depot/test-mercurial-import/file_text+s#1 - opened for add
  //depot/test-mercurial-import/file_text+s2#1 - opened for add
  //depot/test-mercurial-import/file_binary+k#1 - opened for add
  //depot/test-mercurial-import/file_binary+x#1 - opened for add
  //depot/test-mercurial-import/file_binary+kx#1 - opened for add
  //depot/test-mercurial-import/target_symlink+k#1 - opened for add
  //depot/test-mercurial-import/file_symlink+k#1 - opened for add
  //depot/test-mercurial-import/file_ctext#1 - opened for add
  //depot/test-mercurial-import/file_cxtext#1 - opened for add
  //depot/test-mercurial-import/file_ktext#1 - opened for add
  //depot/test-mercurial-import/file_kxtext#1 - opened for add
  //depot/test-mercurial-import/file_ltext#1 - opened for add
  //depot/test-mercurial-import/file_tempobj#1 - opened for add
  //depot/test-mercurial-import/file_ubinary#1 - opened for add
  //depot/test-mercurial-import/file_uxbinary#1 - opened for add
  //depot/test-mercurial-import/file_xbinary#1 - opened for add
  //depot/test-mercurial-import/file_xltext#1 - opened for add
  //depot/test-mercurial-import/file_xtempobj#1 - opened for add
  //depot/test-mercurial-import/file_xtext#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 33 files ...
  add //depot/test-mercurial-import/file_binary#1
  add //depot/test-mercurial-import/file_binary+k#1
  add //depot/test-mercurial-import/file_binary+kx#1
  add //depot/test-mercurial-import/file_binary+x#1
  add //depot/test-mercurial-import/file_ctext#1
  add //depot/test-mercurial-import/file_cxtext#1
  add //depot/test-mercurial-import/file_ktext#1
  add //depot/test-mercurial-import/file_kxtext#1
  add //depot/test-mercurial-import/file_ltext#1
  add //depot/test-mercurial-import/file_symlink#1
  add //depot/test-mercurial-import/file_symlink+k#1
  add //depot/test-mercurial-import/file_tempobj#1
  add //depot/test-mercurial-import/file_text#1
  add //depot/test-mercurial-import/file_text+c#1
  add //depot/test-mercurial-import/file_text+d#1
  add //depot/test-mercurial-import/file_text+f#1
  add //depot/test-mercurial-import/file_text+k#1
  add //depot/test-mercurial-import/file_text+ko#1
  add //depot/test-mercurial-import/file_text+kx#1
  add //depot/test-mercurial-import/file_text+l#1
  add //depot/test-mercurial-import/file_text+m#1
  add //depot/test-mercurial-import/file_text+s#1
  add //depot/test-mercurial-import/file_text+s2#1
  add //depot/test-mercurial-import/file_text+w#1
  add //depot/test-mercurial-import/file_text+x#1
  add //depot/test-mercurial-import/file_ubinary#1
  add //depot/test-mercurial-import/file_uxbinary#1
  add //depot/test-mercurial-import/file_xbinary#1
  add //depot/test-mercurial-import/file_xltext#1
  add //depot/test-mercurial-import/file_xtempobj#1
  add //depot/test-mercurial-import/file_xtext#1
  add //depot/test-mercurial-import/target_symlink#1
  add //depot/test-mercurial-import/target_symlink+k#1
  Change 1 submitted.
  //depot/test-mercurial-import/file_binary+k#1 - refreshing
  //depot/test-mercurial-import/file_binary+kx#1 - refreshing
  //depot/test-mercurial-import/file_ktext#1 - refreshing
  //depot/test-mercurial-import/file_kxtext#1 - refreshing
  //depot/test-mercurial-import/file_symlink+k#1 - refreshing
  //depot/test-mercurial-import/file_text+k#1 - refreshing
  //depot/test-mercurial-import/file_text+ko#1 - refreshing
  //depot/test-mercurial-import/file_text+kx#1 - refreshing

test keyword expansion
  $ p4 edit file_* target_*
  //depot/test-mercurial-import/file_binary#1 - opened for edit
  //depot/test-mercurial-import/file_binary+k#1 - opened for edit
  //depot/test-mercurial-import/file_binary+kx#1 - opened for edit
  //depot/test-mercurial-import/file_binary+x#1 - opened for edit
  //depot/test-mercurial-import/file_ctext#1 - opened for edit
  //depot/test-mercurial-import/file_cxtext#1 - opened for edit
  //depot/test-mercurial-import/file_ktext#1 - opened for edit
  //depot/test-mercurial-import/file_kxtext#1 - opened for edit
  //depot/test-mercurial-import/file_ltext#1 - opened for edit
  //depot/test-mercurial-import/file_symlink#1 - opened for edit
  //depot/test-mercurial-import/file_symlink+k#1 - opened for edit
  //depot/test-mercurial-import/file_tempobj#1 - opened for edit
  //depot/test-mercurial-import/file_text#1 - opened for edit
  //depot/test-mercurial-import/file_text+c#1 - opened for edit
  //depot/test-mercurial-import/file_text+d#1 - opened for edit
  //depot/test-mercurial-import/file_text+f#1 - opened for edit
  //depot/test-mercurial-import/file_text+k#1 - opened for edit
  //depot/test-mercurial-import/file_text+ko#1 - opened for edit
  //depot/test-mercurial-import/file_text+kx#1 - opened for edit
  //depot/test-mercurial-import/file_text+l#1 - opened for edit
  //depot/test-mercurial-import/file_text+m#1 - opened for edit
  //depot/test-mercurial-import/file_text+s#1 - opened for edit
  //depot/test-mercurial-import/file_text+s2#1 - opened for edit
  //depot/test-mercurial-import/file_text+w#1 - opened for edit
  //depot/test-mercurial-import/file_text+x#1 - opened for edit
  //depot/test-mercurial-import/file_ubinary#1 - opened for edit
  //depot/test-mercurial-import/file_uxbinary#1 - opened for edit
  //depot/test-mercurial-import/file_xbinary#1 - opened for edit
  //depot/test-mercurial-import/file_xltext#1 - opened for edit
  //depot/test-mercurial-import/file_xtempobj#1 - opened for edit
  //depot/test-mercurial-import/file_xtext#1 - opened for edit
  //depot/test-mercurial-import/target_symlink#1 - opened for edit
  //depot/test-mercurial-import/target_symlink+k#1 - opened for edit
  $ for T in $TYPES ; do
  >    T2=`echo $T | tr [:upper:] [:lower:]`
  >    echo '$Id$'       >>file_$T2
  >    echo '$Header$'   >>file_$T2
  >    echo '$Date$'     >>file_$T2
  >    echo '$DateTime$' >>file_$T2
  >    echo '$Change$'   >>file_$T2
  >    echo '$File$'     >>file_$T2
  >    echo '$Revision$' >>file_$T2
  >    echo '$Header$$Header$Header$' >>file_$T2
  > done

  $ ln -s 'target_$Header$' crazy_symlink+k
  $ p4 add -t symlink+k crazy_symlink+k
  //depot/test-mercurial-import/crazy_symlink+k#1 - opened for add

  $ p4 submit -d keywords
  Submitting change 2.
  Locking 34 files ...
  add //depot/test-mercurial-import/crazy_symlink+k#1
  edit //depot/test-mercurial-import/file_binary#2
  edit //depot/test-mercurial-import/file_binary+k#2
  edit //depot/test-mercurial-import/file_binary+kx#2
  edit //depot/test-mercurial-import/file_binary+x#2
  edit //depot/test-mercurial-import/file_ctext#2
  edit //depot/test-mercurial-import/file_cxtext#2
  edit //depot/test-mercurial-import/file_ktext#2
  edit //depot/test-mercurial-import/file_kxtext#2
  edit //depot/test-mercurial-import/file_ltext#2
  edit //depot/test-mercurial-import/file_symlink#2
  edit //depot/test-mercurial-import/file_symlink+k#2
  edit //depot/test-mercurial-import/file_tempobj#2
  edit //depot/test-mercurial-import/file_text#2
  edit //depot/test-mercurial-import/file_text+c#2
  edit //depot/test-mercurial-import/file_text+d#2
  edit //depot/test-mercurial-import/file_text+f#2
  edit //depot/test-mercurial-import/file_text+k#2
  edit //depot/test-mercurial-import/file_text+ko#2
  edit //depot/test-mercurial-import/file_text+kx#2
  edit //depot/test-mercurial-import/file_text+l#2
  edit //depot/test-mercurial-import/file_text+m#2
  edit //depot/test-mercurial-import/file_text+s#2
  edit //depot/test-mercurial-import/file_text+s2#2
  edit //depot/test-mercurial-import/file_text+w#2
  edit //depot/test-mercurial-import/file_text+x#2
  edit //depot/test-mercurial-import/file_ubinary#2
  edit //depot/test-mercurial-import/file_uxbinary#2
  edit //depot/test-mercurial-import/file_xbinary#2
  edit //depot/test-mercurial-import/file_xltext#2
  edit //depot/test-mercurial-import/file_xtempobj#2
  edit //depot/test-mercurial-import/file_xtext#2
  edit //depot/test-mercurial-import/target_symlink#2
  edit //depot/test-mercurial-import/target_symlink+k#2
  Change 2 submitted.
  //depot/test-mercurial-import/crazy_symlink+k#1 - refreshing
  //depot/test-mercurial-import/file_binary+k#2 - refreshing
  //depot/test-mercurial-import/file_binary+kx#2 - refreshing
  //depot/test-mercurial-import/file_ktext#2 - refreshing
  //depot/test-mercurial-import/file_kxtext#2 - refreshing
  //depot/test-mercurial-import/file_symlink+k#2 - refreshing
  //depot/test-mercurial-import/file_text+k#2 - refreshing
  //depot/test-mercurial-import/file_text+ko#2 - refreshing
  //depot/test-mercurial-import/file_text+kx#2 - refreshing

check keywords in p4
  $ grep -H Header file_*
  file_binary:$Header$
  file_binary:$Header$$Header$Header$
  file_binary+k:$Header: //depot/test-mercurial-import/file_binary+k#2 $
  file_binary+k:$Header: //depot/test-mercurial-import/file_binary+k#2 $$Header: //depot/test-mercurial-import/file_binary+k#2 $Header$
  file_binary+kx:$Header: //depot/test-mercurial-import/file_binary+kx#2 $
  file_binary+kx:$Header: //depot/test-mercurial-import/file_binary+kx#2 $$Header: //depot/test-mercurial-import/file_binary+kx#2 $Header$
  file_binary+x:$Header$
  file_binary+x:$Header$$Header$Header$
  file_ctext:$Header$
  file_ctext:$Header$$Header$Header$
  file_cxtext:$Header$
  file_cxtext:$Header$$Header$Header$
  file_ktext:$Header: //depot/test-mercurial-import/file_ktext#2 $
  file_ktext:$Header: //depot/test-mercurial-import/file_ktext#2 $$Header: //depot/test-mercurial-import/file_ktext#2 $Header$
  file_kxtext:$Header: //depot/test-mercurial-import/file_kxtext#2 $
  file_kxtext:$Header: //depot/test-mercurial-import/file_kxtext#2 $$Header: //depot/test-mercurial-import/file_kxtext#2 $Header$
  file_ltext:$Header$
  file_ltext:$Header$$Header$Header$
  file_symlink:$Header$
  file_symlink:$Header$$Header$Header$
  file_symlink+k:$Header$
  file_symlink+k:$Header$$Header$Header$
  file_tempobj:$Header$
  file_tempobj:$Header$$Header$Header$
  file_text:$Header$
  file_text:$Header$$Header$Header$
  file_text+c:$Header$
  file_text+c:$Header$$Header$Header$
  file_text+d:$Header$
  file_text+d:$Header$$Header$Header$
  file_text+f:$Header$
  file_text+f:$Header$$Header$Header$
  file_text+k:$Header: //depot/test-mercurial-import/file_text+k#2 $
  file_text+k:$Header: //depot/test-mercurial-import/file_text+k#2 $$Header: //depot/test-mercurial-import/file_text+k#2 $Header$
  file_text+ko:$Header: //depot/test-mercurial-import/file_text+ko#2 $
  file_text+ko:$Header: //depot/test-mercurial-import/file_text+ko#2 $$Header: //depot/test-mercurial-import/file_text+ko#2 $Header$
  file_text+kx:$Header: //depot/test-mercurial-import/file_text+kx#2 $
  file_text+kx:$Header: //depot/test-mercurial-import/file_text+kx#2 $$Header: //depot/test-mercurial-import/file_text+kx#2 $Header$
  file_text+l:$Header$
  file_text+l:$Header$$Header$Header$
  file_text+m:$Header$
  file_text+m:$Header$$Header$Header$
  file_text+s:$Header$
  file_text+s:$Header$$Header$Header$
  file_text+s2:$Header$
  file_text+s2:$Header$$Header$Header$
  file_text+w:$Header$
  file_text+w:$Header$$Header$Header$
  file_text+x:$Header$
  file_text+x:$Header$$Header$Header$
  file_ubinary:$Header$
  file_ubinary:$Header$$Header$Header$
  file_uxbinary:$Header$
  file_uxbinary:$Header$$Header$Header$
  file_xbinary:$Header$
  file_xbinary:$Header$$Header$Header$
  file_xltext:$Header$
  file_xltext:$Header$$Header$Header$
  file_xtempobj:$Header$
  file_xtempobj:$Header$$Header$Header$
  file_xtext:$Header$
  file_xtext:$Header$$Header$Header$

convert
  $ hg convert -s p4 $DEPOTPATH dst
  initializing destination dst repository
  reading p4 views
  collecting p4 changelists
  1 initial
  2 keywords
  scanning source...
  sorting...
  converting...
  1 initial
  0 keywords
  $ hg -R dst log --template 'rev={rev} desc="{desc}" tags="{tags}" files="{files}"\n'
  rev=1 desc="keywords" tags="tip" files="crazy_symlink+k file_binary file_binary+k file_binary+kx file_binary+x file_ctext file_cxtext file_ktext file_kxtext file_ltext file_tempobj file_text file_text+c file_text+d file_text+f file_text+k file_text+ko file_text+kx file_text+l file_text+m file_text+s file_text+s2 file_text+w file_text+x file_ubinary file_uxbinary file_xbinary file_xltext file_xtempobj file_xtext target_symlink target_symlink+k"
  rev=0 desc="initial" tags="" files="file_binary file_binary+k file_binary+kx file_binary+x file_ctext file_cxtext file_ktext file_kxtext file_ltext file_symlink file_symlink+k file_text file_text+c file_text+d file_text+f file_text+k file_text+ko file_text+kx file_text+l file_text+m file_text+s2 file_text+w file_text+x file_ubinary file_uxbinary file_xbinary file_xltext file_xtext target_symlink target_symlink+k"

revision 0
  $ hg -R dst update 0
  30 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ head dst/file_* | cat -v
  ==> dst/file_binary <==
  this is binary
  ==> dst/file_binary+k <==
  this is binary+k
  ==> dst/file_binary+kx <==
  this is binary+kx
  ==> dst/file_binary+x <==
  this is binary+x
  ==> dst/file_ctext <==
  this is ctext
  
  ==> dst/file_cxtext <==
  this is cxtext
  
  ==> dst/file_ktext <==
  this is ktext
  
  ==> dst/file_kxtext <==
  this is kxtext
  
  ==> dst/file_ltext <==
  this is ltext
  
  ==> dst/file_symlink <==
  this is target symlink
  
  ==> dst/file_symlink+k <==
  this is target symlink+k
  
  ==> dst/file_text <==
  this is text
  
  ==> dst/file_text+c <==
  this is text+C
  
  ==> dst/file_text+d <==
  this is text+D
  
  ==> dst/file_text+f <==
  this is text+F
  
  ==> dst/file_text+k <==
  this is text+k
  
  ==> dst/file_text+ko <==
  this is text+ko
  
  ==> dst/file_text+kx <==
  this is text+kx
  
  ==> dst/file_text+l <==
  this is text+l
  
  ==> dst/file_text+m <==
  this is text+m
  
  ==> dst/file_text+s2 <==
  this is text+S2
  
  ==> dst/file_text+w <==
  this is text+w
  
  ==> dst/file_text+x <==
  this is text+x
  
  ==> dst/file_ubinary <==
  this is ubinary
  
  ==> dst/file_uxbinary <==
  this is uxbinary
  
  ==> dst/file_xbinary <==
  this is xbinary
  
  ==> dst/file_xltext <==
  this is xltext
  
  ==> dst/file_xtext <==
  this is xtext

revision 1
  $ hg -R dst update 1
  32 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ head dst/file_* | cat -v
  ==> dst/file_binary <==
  this is binary$Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_binary+k <==
  this is binary+k$Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_binary+kx <==
  this is binary+kx$Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_binary+x <==
  this is binary+x$Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_ctext <==
  this is ctext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_cxtext <==
  this is cxtext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_ktext <==
  this is ktext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_kxtext <==
  this is kxtext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_ltext <==
  this is ltext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_symlink <==
  this is target symlink
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_symlink+k <==
  this is target symlink+k
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_tempobj <==
  this is tempobj
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text <==
  this is text
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+c <==
  this is text+C
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+d <==
  this is text+D
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+f <==
  this is text+F
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+k <==
  this is text+k
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+ko <==
  this is text+ko
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+kx <==
  this is text+kx
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+l <==
  this is text+l
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+m <==
  this is text+m
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+s <==
  this is text+S
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+s2 <==
  this is text+S2
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+w <==
  this is text+w
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_text+x <==
  this is text+x
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_ubinary <==
  this is ubinary
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_uxbinary <==
  this is uxbinary
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_xbinary <==
  this is xbinary
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_xltext <==
  this is xltext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_xtempobj <==
  this is xtempobj
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$
  
  ==> dst/file_xtext <==
  this is xtext
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Header$$Header$Header$

crazy_symlink
  $ readlink crazy_symlink+k
  target_$Header: //depot/test-mercurial-import/crazy_symlink+k#1 $
  $ readlink dst/crazy_symlink+k
  target_$Header$

exit trap:
  stopping the p4 server
