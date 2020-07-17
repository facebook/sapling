  $ configure modern
  $ newserver master

Test debugcauserusterror error formatting
  $ hg debugcauserusterror
  abort: intentional error for debugging with message 'intentional_error'
  [255]
  $ hg debugcauserusterror --traceback
  abort: intentional error for debugging with message 'intentional_error'
  
  error tags: error has type name "taggederror::IntentionalError", error is request issue
  [255]
