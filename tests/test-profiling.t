  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/profiling.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $TESTTMP/logui.py << EOF
  > def uisetup(ui):
  >     class uilogger(ui.__class__):
  >         def log(self, event, *msg, **opts):
  >             self.write(event + str(sorted(opts.keys())) + '\n')
  >             super(uilogger, self).log(event, *msg, **opts)
  >     ui.__class__ = uilogger
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > profiling=$TESTTMP/profiling.py
  > logui=$TESTTMP/logui.py
  > EOF

Test any command produces profiling output
  $ hg init repo
  command[]
  profiletime['interactive_time', 'internal_time']
  commandfinish[]
  $ cd repo
  $ hg status
  command[]
  profiletime['interactive_time', 'internal_time']
  commandfinish[]
