
#require eden

test eden commands help

  $ eden --help > /dev/null
  $ eden du --help > /dev/null
  $ eden imnotacommandiswear --help > /dev/null 2>&1
  [64]

Test a few simple eden commands

   $ eden status
   EdenFS is running normally \(pid ([0-9]+)\) (re)
   $ eden list
   $ eden version
   Installed: -
   Running:   -
   (Dev version of EdenFS seems to be running)

Make sure local config values are picked up
  $ cat > $HOME/.edenrc <<EOF
  > [doctor]
  > HOME_bogus = "HOME is TESTTMP"
  > EOF
  $ cat >> $TESTTMP/.edenrc <<EOF
  > TESTTMP_bogus = "TESTTMP is HOME"
  > EOF
  $ eden config | grep _bogus
  HOME_bogus = "HOME is TESTTMP"
  TESTTMP_bogus = "TESTTMP is HOME"
