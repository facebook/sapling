#require py2
  $ configure modern
  $ newserver master

Test debugthrowrustexception error formatting
  $ hg debugthrowrustexception
  abort: intentional error for debugging with message 'intentional_error'
  [255]
  $ hg debugthrowrustexception --traceback
  abort: intentional error for debugging with message 'intentional_error'
  
  Tags: error is marked with typename "hgcommands::commands::IntentionalError", error is marked as user's fault
  [255]
