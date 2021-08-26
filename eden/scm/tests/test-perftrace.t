#require no-fsmonitor

  $ setconfig tracing.threshold=0
  $ newrepo
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ TRACING_DATA_FAKE_CLOCK=5000 hg up $C --config tracing.stderr=True 2> trace
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sed 's/line [0-9]*$/_/' trace
  Process _ Thread _:
  Start Dur.ms | Name * (glob)
      5    ... | Run Command * (glob)
               | - pid = 0 * (glob)
               | - uid = 0 * (glob)
               | - nice = 0 * (glob)
               | - args = * (glob)
               | - parent_pids = [] * (glob)
               | - parent_names = [] * (glob)
               | - version = * (glob)
               | - exit_code = 0 * (glob)
               | - max_rss = 0 * (glob)
     10     +5  \ Initialize Python * (glob)
     20     +5  \ import edenscm * (glob)
  
  Process _ Thread _:
  Start Dur.ms | Name                             Source
     30   +165 | Main Python Command              (perftrace)
     35     +5  \ Repo Setup                      edenscm.mercurial.hg line _
                 | - local = true                 :
     45   +145  \ Update                          edenscm.mercurial.util line _
     50   +135   | Timed Function: mergeupdate    edenscm.mercurial.merge line _
     55    +35    \ Status                        edenscm.mercurial.dirstate line _
                   | - A/M/R Files = 0            :
     60     +5     \ Timed Function: fswalk       edenscm.mercurial.filesystem line _
     70     +5     \ _rustwalk.next               (generator)
     80     +5     \ _rustwalk.next               (generator)
     95    +45    \ Progress Bar: calculating     (progressbar)
    100    +35     | Calculate Updates            edenscm.mercurial.merge line _
    105    +15      \ Manifest Diff               (perftrace)
                     | - Differences = 3          :
                     | - Tree Fetches = 0         :
    125     +5      \ Check Unknown Files         edenscm.mercurial.merge line _
    145    +25    \ Apply Updates                 edenscm.mercurial.util line _
                   | - Actions = 3                :
                   | - Disk Writes = 3            :
                   | - Deleted Files = 0          :
                   | - Written Files = 3          :
    150    +15     | Timed Function: applyupdates edenscm.mercurial.merge line _
    155     +5     | Progress Bar: updating       (progressbar)
                   | - total = 3                  :
    175     +5    \ Progress Bar: recording       (progressbar)
                   | - total = 3                  :
  
  Process _ Thread _:
  Start Dur.ms | Name               Source
    110     +5 | Get Missing        revisionstore::contentstore _
               | - keys = 1         :
  
  
