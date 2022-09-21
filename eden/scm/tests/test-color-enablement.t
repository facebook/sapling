#chg-compatible
#debugruntest-compatible

  $ setconfig alias.testcolor="debugtemplate '{label(\"green\", \"output\n\")}'"

  $ HGPLAINEXCEPT=alias hg testcolor
  output

  $ HGPLAINEXCEPT=alias hg testcolor --color always
  output

  $ hg testcolor --color always
  \x1b[32moutput\x1b[39m (esc)

  $ hg testcolor --color yes
  \x1b[32moutput\x1b[39m (esc)

  $ hg testcolor --color auto
  output

  $ HGPLAINEXCEPT=color,alias hg testcolor --color always
  \x1b[32moutput\x1b[39m (esc)

  $ hg testcolor --config ui.color=always
  \x1b[32moutput\x1b[39m (esc)

  $ hg testcolor --config ui.color=t
  output
