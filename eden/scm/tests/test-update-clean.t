#debugruntest-compatible

  $ enable remotefilelog

  $ newrepo foo
  $ echo remotefilelog >> .hg/requires

  $ echo a > a
  $ hg commit -qAm init

Make sure merge state is cleared when we have a clean tree.
  $ mkdir .hg/merge

    # Write out some valid contents
    with open(f"foo/.hg/merge/state2", "bw") as f:
        f.write(b"L\x00\x00\x00\x28")
        f.write(b"a" * 40)

  $ hg debugmergestate
  local: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg up -qC . --config experimental.nativecheckout=True
  $ hg debugmergestate
  no merge state found
