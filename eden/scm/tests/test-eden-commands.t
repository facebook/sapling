
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

test rust-python command routing

  $ newclientrepo
  $ eden prefetch-profile aactivate
  usage: edenfsctl [-h] [--config-dir CONFIG_DIR] [--etc-eden-dir ETC_EDEN_DIR]
                   [--home-dir HOME_DIR] [--version] [--debug]
                   COMMAND ...
  edenfsctl: error: unrecognized arguments: aactivate
  [64]
  $ EDENFSCTL_SKIP_RUST=1 eden prefetch-profile activate
  usage: edenfsctl [-h] [--config-dir CONFIG_DIR] [--etc-eden-dir ETC_EDEN_DIR]
                   [--home-dir HOME_DIR] [--version] [--debug]
                   COMMAND ...
  edenfsctl: error: unrecognized arguments: activate
  [64]
  $ eden prefetch-profile activate
  error: The following required arguments were not provided:
      <PROFILE_NAME>
  * (glob)
  USAGE:
  * (glob)
  * (glob)
  For more information try --help
  [64]


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
