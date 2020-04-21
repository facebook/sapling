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
  Start Dur.ms | Name                                       Source
      5    ... | Run Command                                hgcommands::run _
               | - pid = 0                                  :
               | - uid = 0                                  :
               | - nice = 0                                 :
               | - args = ["hg","up","26805aba1e600a82e9... :
               | - parent_pids = []                         :
               | - parent_names = []                        :
               | - exit_code = 0                            :
               | - max_rss = 0                              :
     10     +5  \ Initialize Python                         hgcommands::hgpython _
     20     +5  \ import edenscm                            hgcommands::hgpython _
     30   +145  \ Main Python Command                       (perftrace)
     35     +5   \ Repo Setup                               edenscm.mercurial.hg line _
                  | - flag = local                          :
     45   +125   \ Update                                   edenscm.mercurial.util line _
     50   +115    | Timed Function: mergeupdate             edenscm.mercurial.merge line _
     55    +35     \ Status                                 edenscm.mercurial.dirstate line _
                    | - A/M/R Files = 0                     :
     60     +5      \ Timed Function: fswalk                edenscm.mercurial.filesystem line _
     70     +5      \ _rustwalk.next                        (generator)
     80     +5      \ _rustwalk.next                        (generator)
     95    +25     \ Progress Bar: calculating              (progressbar)
    100    +15      | Calculate Updates                     edenscm.mercurial.merge line _
    105     +5      | Check Unknown Files                   edenscm.mercurial.merge line _
    125    +25     \ Apply Updates                          edenscm.mercurial.util line _
                    | - Actions = 3                         :
                    | - Disk Writes = 3                     :
                    | - Deleted Files = 0                   :
                    | - Written Files = 3                   :
    130    +15      | Timed Function: applyupdates          edenscm.mercurial.merge line _
    135     +5      | Progress Bar: updating                (progressbar)
                    | - total = 3                           :
    155     +5     \ Progress Bar: recording                (progressbar)
                    | - total = 3                           :
  
  
