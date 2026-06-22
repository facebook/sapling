  $ enable smartlog

Prepare a large repo:

  $ cd
  $ sl init --config format.use-virtual-repo-with-size-factor=12 virtual12
  $ cd virtual12
  $ sl
  o  commit:      cc0030700700
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Sun Oct 07 08:25:23 2029 +0000
     summary:     synthetic commit 124792833

Large limit() slices stay lazy and should not timeout:

  $ sl go null -q
  $ CODING_AGENT_METADATA=id=test_agent sl log -r 'limit(public() & author(test), 10000000)' -T '{node}\n' | wc -l
  100000
  abort: revset query scanned over 100000 commits
  (run 'sl help agent performance' for guidance.)
