#chg-compatible
#debugruntest-compatible

  $ setconfig ui.color=always color.mode=ansi
  $ setconfig color.color.none=0

  $ setconfig alias.testcolor="debugtemplate '{label(\"test.test\", \"output\n\")}'"

  $ hg testcolor --config color.test.test=blue
  \x1b[0;34moutput\x1b[0m (esc)

  $ hg testcolor --config color.test.test="blue bold"
  \x1b[0;34;1moutput\x1b[0m (esc)

  $ hg testcolor --config color.test.test="brightblue"
  \x1b[0;94moutput\x1b[0m (esc)

  $ hg testcolor --config color.test.test="blue+bold"
  \x1b[0;34;1moutput\x1b[0m (esc)

  $ hg testcolor --config color.test.test="brightblue:blue+bold"
  \x1b[0;94moutput\x1b[0m (esc)

  $ HGCOLORS=8 hg testcolor --config color.test.test="brightblue:blue+bold"
  \x1b[0;34;1moutput\x1b[0m (esc)

  $ hg testcolor --config color.test.test="brightblue:blue+bold underline"
  \x1b[0;94;4moutput\x1b[0m (esc)

  $ HGCOLORS=8 hg testcolor --config color.test.test="brightblue:blue+bold underline"
  \x1b[0;34;1;4moutput\x1b[0m (esc)

  $ HGCOLORS=16777216 hg testcolor --config color.test.test="#359:color68+italic:brightblue:blue+bold underline"
  \x1b[0;38;2;51;85;153;4moutput\x1b[0m (esc)

  $ HGCOLORS=256 hg testcolor --config color.test.test="#359:color68+italic:brightblue:blue+bold underline"
  \x1b[0;38;5;68;3;4moutput\x1b[0m (esc)

  $ HGCOLORS=16 hg testcolor --config color.test.test="#359:color68+italic:brightblue:blue+bold underline"
  \x1b[0;94;4moutput\x1b[0m (esc)

  $ HGCOLORS=8 hg testcolor --config color.test.test="#359:color68+italic:brightblue:blue+bold underline"
  \x1b[0;34;1;4moutput\x1b[0m (esc)
