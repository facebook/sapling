import datetime
import time

from edenscm.mercurial import util


def extsetup(ui):
    fakedate = ui.config("fakedate", "date", "1996-03-07 14:00:01Z")
    fakedate = util.parsedate(fakedate)[0]

    class fakedatetime(datetime.datetime):
        @staticmethod
        def now(tz=None):
            return datetime.datetime.fromtimestamp(fakedate, tz)

    datetime.datetime = fakedatetime

    def faketime():
        return fakedate

    time.time = faketime
