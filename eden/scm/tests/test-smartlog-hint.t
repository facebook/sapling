
  $ enable smartlog

  $ configure modern
  $ newrepo

  $ setconfig commands.naked-default.in-repo=sl
  $ cat >> $HGRCPATH << EOF
  > [hint]
  > %unset ack
  > EOF

  $ sl sl
  hint[smartlog-default-command]: you can run smartlog with simply `sl`
  hint[hint-ack]: use 'sl hint --ack smartlog-default-command' to silence these hints

  $ sl smartlog
  hint[smartlog-default-command]: you can run smartlog with simply `sl`
  hint[hint-ack]: use 'sl hint --ack smartlog-default-command' to silence these hints

  $ sl
  $ sl sl -T '{ssl}'

  $ setconfig commands.naked-default.in-repo=version
  $ sl sl
