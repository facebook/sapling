Test raw style of hgweb

  $ hg init test
  $ cd test
  $ mkdir sub
  $ cat >'sub/some "text".txt' <<ENDSOME
  > This is just some random text
  > that will go inside the file and take a few lines.
  > It is very boring to read, but computers don't
  > care about things like that.
  > ENDSOME
  $ hg add 'sub/some "text".txt'
  $ hg commit -d "1 0" -m "Just some text"

  $ hg serve -p $HGPORT -A access.log -E error.log -d --pid-file=hg.pid

  $ cat hg.pid >> $DAEMON_PIDS
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/?f=a23bf1310f6e;file=sub/some%20%22text%22.txt;style=raw' content-type content-length content-disposition) >getoutput.txt &
  $ sleep 5
  $ kill `cat hg.pid`
  $ sleep 1 # wait for server to scream and die
  $ cat getoutput.txt
  200 Script output follows
  content-type: text/plain; charset="ascii"
  content-length: 157
  content-disposition: inline; filename="some \"text\".txt"
  
  This is just some random text
  that will go inside the file and take a few lines.
  It is very boring to read, but computers don't
  care about things like that.
  $ cat access.log error.log
  127.0.0.1 - - [*] "GET /?f=a23bf1310f6e;file=sub/some%20%22text%22.txt;style=raw HTTP/1.1" 200 - (glob)

