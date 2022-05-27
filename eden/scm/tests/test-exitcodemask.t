#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Command line flag is effective:

  $ hg add a --config 'ui.exitcodemask=63'
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [63]

  $ HGPLAIN=1 hg add a --config 'ui.exitcodemask=63'
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [63]

# Config files are ignored if HGPLAIN is set:

  $ setconfig 'ui.exitcodemask=31'
  $ hg add a
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [31]

  $ HGPLAIN=1 hg add a
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

# But HGPLAINEXCEPT can override the behavior:

  $ HGPLAIN=1 HGPLAINEXCEPT=exitcode hg add a
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [31]
