  $ echo 'raise Exception("bit bucket overflow")' > badext.py
  $ abspath=`pwd`/badext.py

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg =
  > hgext.gpg =
  > badext = $abspath
  > badext2 =
  > EOF

  $ hg -q help help
  *** failed to import extension badext from $TESTTMP/badext.py: bit bucket overflow
  *** failed to import extension badext2: No module named badext2
  hg help [-ec] [TOPIC]
  
  show help for a given topic or a help overview
