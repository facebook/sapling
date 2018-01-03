  > echo "[extensions]" >> $HGRCPATH
  > echo "remotenames=`dirname $TESTDIR`/remotenames.py" >> $HGRCPATH
  > echo "strip=" >> $HGRCPATH

Test that hg strip -B stops at remotenames
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ echo a > a
  $ hg commit -Aqm aa
  $ hg phase -p
  $ hg push -q --to master --create
  $ echo b > b
  $ hg commit -Aqm bb
  $ hg book foo
  $ hg strip -qB foo
  bookmark 'foo' deleted
  $ hg log --template "{desc}\n"
  aa
  a

Test that hg strip -B deletes bookmark even if there is a remote bookmark
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ hg phase -p
  $ hg push -q --to master --create
  $ hg book foo
  $ hg strip -qB foo
  bookmark 'foo' deleted
  $ hg log

