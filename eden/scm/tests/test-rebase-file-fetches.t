  $ enable amend morestatus rebase
  $ setconfig rebase.experimental.inmemory=true
  $ setconfig drawdag.defaultfiles=false

  $ newserver server
  $ drawdag <<EOS
  >      # C/four = four
  >      # B/two = 2
  > C B  # B/three = three
  > |/   # B/one = (removed)
  > A    # A/one = one
  >      # A/two = two
  > EOS

  $ newclientrepo client test:server
  $ LOG=file_fetches=trace hg rebase -q -r $B -d $C
  TRACE file_fetches: attrs=["header"] keys=["three"]
  TRACE file_fetches: attrs=["history"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header", "aux"] keys=["three", "two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
  TRACE file_fetches: attrs=["history"] keys=["two"]
  TRACE file_fetches: attrs=["content", "header"] keys=["three"]
  TRACE file_fetches: attrs=["content", "header"] keys=["two"]
