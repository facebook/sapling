#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ newserver server
  $ clone server client
  $ cd client

Test pushing of specific sets of commits
  $ touch xxx
  $ hg add xxx
  $ hg debughiddencommit -q
  15f9adf02fffc4fac0167c73bd193dd346752d08
  $ hg show 15f9adf02fffc4fac0167c73bd193dd346752d08
  commit:      15f9adf02fff
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       xxx
  description:
  Ephemeral commit
  
  
  
