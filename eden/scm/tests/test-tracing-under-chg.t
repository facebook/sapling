#debugruntest-compatible
#chg-compatible
#require chg no-eden

Use any random Python command - make sure we see tracing events.
  $ LOG=commands=info hg hint
  * commands::run: enter (glob)
  * commands::hgpython: enter (glob)
  * commands::hgpython: exit (glob)
  * commands::run: exit (glob)

  $ LOG=commands=info hg hint
  * commands::run: enter (glob)
  * commands::hgpython: enter (glob)
  * commands::hgpython: exit (glob)
  * commands::run: exit (glob)
