#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ enable rebase

  $ newserver server
  $ drawdag <<'EOS'
  > E   # E/y = 1\n (renamed from x)
  > |
  > D   # D/d1 = 1\n
  > |
  > C   # C/c1 = 1\n
  > |
  > | B # B/x = 2\n
  > | |
  > |/
  > A   # A/x = 1\n
  >     # bookmark master = E
  > EOS

  $ newclientrepo client server
  $ sl pull -q -r $B

Fetch the trees of A, B and E. Rebase needs them to merge.
  $ sl go -q $A
  $ sl go -q $E
  $ sl go -q $B

Copy tracing reads the trees of C and D. The client does not have them. The failpoint makes the tree fetch fail.
The conflict hint also traces the rename. In tests, a trace error in the hint stops the command. Thus, turn off the hint.
  $ setconfig copytrace.hint-with-commit=false
  $ FAILPOINTS=eagerepo::api::trees=return SL_LOG=copytrace=warn sl rebase -r $B -d $E --config devel.print-metrics=copytrace
  rebasing 98114c1b9d02 "B"
   WARN copytrace::copy_trace: failed to trace rename src=98114c1b9d02bfb676c60a080316fdc825dfe4a9 dst=1ce439dbd9bd25ec9f47996327965e847e5ebc37 src_path=RepoPathBuf("x") err=* (glob)
  
  Caused by:
      0: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      1: * (glob)
      2: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {} (?)
  other [source] changed x which local [dest] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  copytrace_copies: 0
  copytrace_missingfiles: 1
  copytrace_trace_rename_error: 1
  [1]
  $ sl rebase --abort
  rebase aborted

When the tree fetch works, copy tracing finds the rename.
  $ sl rebase -r $B -d $E
  rebasing 98114c1b9d02 "B"
  merging y and x to y
