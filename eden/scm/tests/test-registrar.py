# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import unittest

import edenscm.error as error
import edenscm.registrar as registrar
import silenttestrunner

command = registrar.command(dict())


@command("notacommand")
def thisisnotacommand(_ui, **opts):
    "yadayadayada"
    return "foo"


command._table["commandstring"] = "Rust commands are actually registered as strings"


class TestRegistrar(unittest.TestCase):
    def testnohelptext(self):
        # Redifining commands should not be an issue as long as they
        # do not have any documentation
        @command("notacommand")
        def thisalreadyexisted(_ui, **opts):
            return "foobar"

        self.assertEqual(thisalreadyexisted(None), "foobar")

    def testhelptextchangefailure(self):
        "Assert that we cannot change the help text"
        with self.assertRaises(error.ProgrammingError) as err:

            @command("notacommand")
            def helpredefiner():
                "this is the help text for not a command"
                pass

        self.assertEqual(
            str(err.exception), 'duplicate help message for name: "notacommand"'
        )

    def testsynopsischangefailure(self):
        "Assert that we cannot change the synopsis"
        with self.assertRaises(error.ProgrammingError) as err:

            @command("commandstring", synopsis="this is the new synopsis")
            def helpredefiner2():
                pass

        self.assertEqual(
            str(err.exception), 'duplicate help message for name: "commandstring"'
        )


if __name__ == "__main__":
    silenttestrunner.main(__name__)
