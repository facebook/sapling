Environment setup for MQ

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg init
  $ hg qinit

Should fail if no patches applied

  $ hg qrefresh
  no patches applied
  [1]
  $ hg qrefresh -e
  no patches applied
  [1]
  $ hg qnew -m "First commit message" first-patch
  $ echo aaaa > file
  $ hg add file
  $ hg qrefresh

Should display 'First commit message'

  $ hg log -l1 --template "{desc}\n"
  First commit message

Testing changing message with -m

  $ echo bbbb > file
  $ hg qrefresh -m "Second commit message"

Should display 'Second commit message'

  $ hg log -l1 --template "{desc}\n"
  Second commit message

Testing changing message with -l

  $ echo "Third commit message" > logfile
  $ echo " This is the 3rd log message" >> logfile
  $ echo bbbb > file
  $ hg qrefresh -l logfile

Should display 'Third commit message\\\n This is the 3rd log message'

  $ hg log -l1 --template "{desc}\n"
  Third commit message
   This is the 3rd log message

Testing changing message with -l-

  $ hg qnew -m "First commit message" second-patch
  $ echo aaaa > file2
  $ hg add file2
  $ echo bbbb > file2
  $ (echo "Fifth commit message"; echo " This is the 5th log message") | hg qrefresh -l-

Should display 'Fifth commit message\\\n This is the 5th log message'

  $ hg log -l1 --template "{desc}\n"
  Fifth commit message
   This is the 5th log message
