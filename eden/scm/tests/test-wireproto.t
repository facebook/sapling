#chg-compatible

  $ setconfig extensions.treemanifest=!
#require killdaemons

Test wire protocol argument passing

Setup repo:

  $ hg init repo

Local:

  $ hg debugwireargs repo eins zwei --three drei --four vier
  eins zwei drei vier None
  $ hg debugwireargs repo eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs repo eins zwei
  eins zwei None None None
  $ hg debugwireargs repo eins zwei --five fuenf
  eins zwei None None fuenf

HTTP:

  $ hg  serve -R repo -p $HGPORT -d --pid-file=hg1.pid \
  >   -E error.log -A access.log \
  >   --config experimental.httppostargs=yes
  $ cat hg1.pid >> $DAEMON_PIDS

  $ hg debugwireargs http://localhost:$HGPORT/ un deux trois quatre
  un deux trois quatre None
  $ hg debugwireargs http://localhost:$HGPORT/ \ un deux trois\  qu\ \ atre
   un deux trois  qu  atre None
  $ hg debugwireargs http://localhost:$HGPORT/ eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs http://localhost:$HGPORT/ eins zwei
  eins zwei None None None
  $ hg debugwireargs http://localhost:$HGPORT/ eins zwei --five fuenf
  eins zwei None None None
  $ hg debugwireargs http://localhost:$HGPORT/ un deux trois onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  un deux trois onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx None
  $ cat error.log
  $ cat access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:39* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:39* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:43* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:43* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:27* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:27* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:1033* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:1033* (glob)

HTTP without args-in-POST:
  $ hg serve -R repo -p 0 --port-file $TESTTMP/.port -d --pid-file=hg1.pid -E error.log -A access.log
  $ HGPORT1=`cat $TESTTMP/.port`
  $ cat hg1.pid >> $DAEMON_PIDS

  $ hg debugwireargs http://localhost:$HGPORT1/ un deux trois quatre
  un deux trois quatre None
  $ hg debugwireargs http://localhost:$HGPORT1/ \ un deux trois\  qu\ \ atre
   un deux trois  qu  atre None
  $ hg debugwireargs http://localhost:$HGPORT1/ eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs http://localhost:$HGPORT1/ eins zwei
  eins zwei None None None
  $ hg debugwireargs http://localhost:$HGPORT1/ eins zwei --five fuenf
  eins zwei None None None
  $ hg debugwireargs http://localhost:$HGPORT1/ un deux trois onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  un deux trois onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx None
  $ cat error.log
  $ cat access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:39* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:39* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:43* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:43* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:27* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:27* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:17* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:1033* (glob)
  * - - [*] "POST /?cmd=debugwireargs HTTP/1.1" 200 - x-hgargs-post:1033* (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=quatre&one=un&three=trois&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=quatre&one=un&three=trois&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=qu++atre&one=+un&three=trois+&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=qu++atre&one=+un&three=trois+&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=vier&one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=vier&one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:one=eins&two=zwei x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx&one x-hgarg-2:=un&three=trois&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs HTTP/1.1" 200 - x-hgarg-1:four=onethousandcharactersxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx&one x-hgarg-2:=un&three=trois&two=deux x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)

HTTP without the httpheader capability:

  $ HGRCPATH="`pwd`/repo/.hgrc"
  $ export HGRCPATH
  $ CAP=httpheader
  $ . "$TESTDIR/notcapable"

  $ hg serve -R repo -p 0 --port-file $TESTTMP/.port -d --pid-file=hg2.pid -E error2.log -A access2.log
  $ HGPORT2=`cat $TESTTMP/.port`
  $ cat hg2.pid >> $DAEMON_PIDS

  $ hg debugwireargs http://localhost:$HGPORT2/ un deux trois quatre
  un deux trois quatre None
  $ hg debugwireargs http://localhost:$HGPORT2/ eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs http://localhost:$HGPORT2/ eins zwei
  eins zwei None None None
  $ hg debugwireargs http://localhost:$HGPORT2/ eins zwei --five fuenf
  eins zwei None None None
  $ cat error2.log
  $ cat access2.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&four=quatre&one=un&three=trois&two=deux HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&four=quatre&one=un&three=trois&two=deux HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&four=vier&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&four=vier&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)
  $LOCALIP - - [$LOGDATE$] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=$USUAL_COMPRESSIONS$ (glob)

SSH (try to exercise the ssh functionality with a dummy script):

  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo uno due tre quattro
  uno due tre quattro None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei
  eins zwei None None None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei --five fuenf
  eins zwei None None None

Explicitly kill daemons to let the test exit on Windows

  $ killdaemons.py

