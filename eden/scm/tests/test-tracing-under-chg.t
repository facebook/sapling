#chg-compatible
#require chg

Use any random Python command - make sure we see tracing events.
  $ LOG=hgcommands=info hg hint
  * hgcommands::run: enter (glob)
  * hgcommands::hgpython: enter (glob)
  * hgcommands::hgpython: exit (glob)
  * hgcommands::run: exit (glob)

  $ LOG=hgcommands=info hg hint
  * hgcommands::run: enter (glob)
  * hgcommands::hgpython: enter (glob)
  * hgcommands::hgpython: exit (glob)
  * hgcommands::run: exit (glob)
