Command line flag is effective:

  $ hg add a --config ui.exitcodemask=63
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [63]

  $ HGPLAIN=1 hg add a --config ui.exitcodemask=63
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [63]

Config files are ignored if HGPLAIN is set:

  $ setconfig ui.exitcodemask=31
  $ hg add a
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [31]

  $ HGPLAIN=1 hg add a
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]

But HGPLAINEXCEPT can override the behavior:

  $ HGPLAIN=1 HGPLAINEXCEPT=exitcode hg add a
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [31]
