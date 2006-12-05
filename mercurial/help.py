# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

helptable = {
    "dates|Date Formats":
    r'''
    Some commands (backout, commit, tag) allow the user to specify a date.
    Possible formats for dates are:

YYYY-mm-dd \HH:MM[:SS] [(+|-)NNNN]::
    This is a subset of ISO 8601, allowing just the recommended notations
    for date and time. The last part represents the timezone; if omitted,
    local time is assumed. Examples:

    "2005-08-22 03:27 -0700"

    "2006-04-19 21:39:51"

aaa bbb dd HH:MM:SS YYYY [(+|-)NNNN]::
    This is the date format used by the C library. Here, aaa stands for
    abbreviated weekday name and bbb for abbreviated month name. The last
    part represents the timezone; if omitted, local time is assumed.
    Examples:

    "Mon Aug 22 03:27:00 2005 -0700"

    "Wed Apr 19 21:39:51 2006"

unixtime offset::
    This is the internal representation format for dates. unixtime is
    the number of seconds since the epoch (1970-01-01 00:00 UTC). offset
    is the offset of the local timezone, in seconds west of UTC (negative
    if the timezone is east of UTC).
    Examples:

    "1124706420 25200" (2005-08-22 03:27:00 -0700)

    "1145475591 -7200" (2006-04-19 21:39:51 +0200)
    ''',
}

