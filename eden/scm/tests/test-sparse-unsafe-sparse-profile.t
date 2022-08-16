#chg-compatible
#debugruntest-compatible

test sparse

  $ enable sparse
  $ hg init myrepo
  $ cd myrepo

  $ echo a > show
  $ echo x > hide
  $ hg ci -Aqm 'initial'

  $ echo b > show
  $ echo y > hide
  $ echo aa > show2
  $ echo xx > hide2
  $ hg ci -Aqm 'two'

Verify basic include

  $ hg up -q 'desc(initial)'
  $ hg sparse exclude 'hide'
  $ hg sparse exclude 'show'
  $ hg sparse show
  Additional Excluded Paths:
  
    hide
    show

Verify that reset and include fails because it tries to include bad file
  $ hg sparse reset --config sparse.unsafe_sparse_profile_marker_files="hide" --config sparse.unsafe_sparse_profile_message="msg"
  abort: 'hide' file is included in sparse profile, it might not be safe because it may introduce a large amount of data into your repository
  msg
  (If you are know what you are doing re-run with allow-unsafe-profile-changes, otherwise contact Source control @ fb)
  [255]
  $ hg sparse --config sparse.unsafe_sparse_profile_marker_files="hide, show" include hide
  abort: 'hide' file is included in sparse profile, it might not be safe because it may introduce a large amount of data into your repository
  (If you are know what you are doing re-run with allow-unsafe-profile-changes, otherwise contact Source control @ fb)
  [255]
  $ hg sparse --config sparse.unsafe_sparse_profile_marker_files="hide, show" include show
  abort: 'show' file is included in sparse profile, it might not be safe because it may introduce a large amount of data into your repository
  (If you are know what you are doing re-run with allow-unsafe-profile-changes, otherwise contact Source control @ fb)
  [255]

Including another file is fine
  $ hg sparse --config sparse.unsafe_sparse_profile_marker_files="hide" include show
  $ hg sparse show
  Additional Included Paths:
  
    show
  
  Additional Excluded Paths:
  
    hide

Now allow reset to go through
  $ hg sparse reset --allow-unsafe-profile-changes --config sparse.unsafe_sparse_profile_marker_files="hide"
  hint[sparse-unsafe-profile]: Your sparse profile might be incorrect, and it can lead to downloading too much data and slower mercurial operations.
  hint[hint-ack]: use 'hg hint --ack sparse-unsafe-profile' to silence these hints
  $ hg sparse show
  $ ls
  hide
  show

Make sure we don't get any errors if sparse profile already includes marker file
  $ hg sparse --config sparse.unsafe_sparse_profile_marker_files="hide" exclude show
  hint[sparse-unsafe-profile]: Your sparse profile might be incorrect, and it can lead to downloading too much data and slower mercurial operations.
  hint[hint-ack]: use 'hg hint --ack sparse-unsafe-profile' to silence these hints
  $ ls
  hide
  $ hg st --config sparse.unsafe_sparse_profile_marker_files="hide" --config sparse.unsafe_sparse_profile_message="run 'hg sparse enable profile'"
  hint[sparse-unsafe-profile]: Your sparse profile might be incorrect, and it can lead to downloading too much data and slower mercurial operations.
  run 'hg sparse enable profile'
  hint[hint-ack]: use 'hg hint --ack sparse-unsafe-profile' to silence these hints
