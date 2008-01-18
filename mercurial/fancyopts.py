import getopt

def fancyopts(args, options, state):
    """
    read args, parse options, and store options in state

    each option is a tuple of:

      short option or ''
      long option
      default value
      description

    option types include:

      boolean or none - option sets variable in state to true
      string - parameter string is stored in state
      list - parameter string is added to a list
      integer - parameter strings is stored as int
      function - call function with parameter

    non-option args are returned
    """
    namelist = []
    shortlist = ''
    argmap = {}
    defmap = {}

    for short, name, default, comment in options:
        # convert opts to getopt format
        oname = name
        name = name.replace('-', '_')

        argmap['-' + short] = argmap['--' + oname] = name
        defmap[name] = default

        # copy defaults to state
        if isinstance(default, list):
            state[name] = default[:]
        elif callable(default):
            state[name] = None
        else:
            state[name] = default

        # does it take a parameter?
        if not (default is None or default is True or default is False):
            if short: short += ':'
            if oname: oname += '='
        if short:
            shortlist += short
        if name:
            namelist.append(oname)

    # parse arguments
    opts, args = getopt.getopt(args, shortlist, namelist)

    # transfer result to state
    for opt, val in opts:
        name = argmap[opt]
        t = type(defmap[name])
        if t is type(fancyopts):
            state[name] = defmap[name](val)
        elif t is type(1):
            state[name] = int(val)
        elif t is type(''):
            state[name] = val
        elif t is type([]):
            state[name].append(val)
        elif t is type(None) or t is type(False):
            state[name] = True

    # return unparsed args
    return args
