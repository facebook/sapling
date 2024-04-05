#debugruntest-compatible

#require eden

test eden commands help

  $ eden --help > /dev/null
  $ eden du --help > /dev/null
  $ eden imnotacommandiswear --help > /dev/null 2>&1
  [2]

Test a few simple eden commands

   $ eden status
   EdenFS is running normally \(pid ([0-9]+)\) (re)
   $ eden list
   $ eden version
   Installed: -
   Running:   -
   (Dev version of EdenFS seems to be running)
