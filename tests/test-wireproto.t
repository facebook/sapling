
Test wire protocol argument passing

Setup repo:

  $ hg init repo

Local:

  $ hg debugwireargs repo eins zwei
  eins zwei None None

HTTP:

  $ hg serve -R repo -p $HGPORT -d --pid-file=hg1.pid -E error.log -A access.log
  $ cat hg1.pid >> $DAEMON_PIDS

  $ hg debugwireargs http://localhost:$HGPORT/ eins zwei
  eins zwei None None
  $ cat access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - (glob)
  * - - [*] "GET /?cmd=debugwireargs&one=eins&two=zwei HTTP/1.1" 200 - (glob)

SSH (try to exercise the ssh functionality with a dummy script):

  $ cat <<EOF > dummyssh
  > import sys
  > import os
  > os.chdir(os.path.dirname(sys.argv[0]))
  > if sys.argv[1] != "user@dummy":
  >     sys.exit(-1)
  > if not os.path.exists("dummyssh"):
  >     sys.exit(-1)
  > os.environ["SSH_CLIENT"] = "127.0.0.1 1 2"
  > r = os.system(sys.argv[2])
  > sys.exit(bool(r))
  > EOF

  $ hg debugwireargs --ssh "python ./dummyssh" ssh://user@dummy/repo eins zwei
  eins zwei None None

