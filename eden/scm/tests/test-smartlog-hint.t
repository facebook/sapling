#debugruntest-incompatible
Not using debugruntest to be sure we are testing "real" argv handling.
#chg-compatible

  $ enable smartlog

  $ configure modern
  $ newrepo

  $ setconfig commands.naked-default.in-repo=sl
  $ cat >> $HGRCPATH << EOF
  > [hint]
  > %unset ack
  > EOF

  $ hg sl
  hint[smartlog-default-command]: you can run smartlog with simply `hg`
  hint[hint-ack]: use 'hg hint --ack smartlog-default-command' to silence these hints

  $ hg smartlog
  hint[smartlog-default-command]: you can run smartlog with simply `hg`
  hint[hint-ack]: use 'hg hint --ack smartlog-default-command' to silence these hints

  $ hg
  $ hg sl -T '{ssl}'

  $ setconfig commands.naked-default.in-repo=version
  $ hg sl
