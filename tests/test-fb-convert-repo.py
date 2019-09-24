import os
import unittest

from edenscm.hgext.convert.repo import gitutil


def draft(test_func):
    if os.environ["USER"] not in ("mdevine", "tch"):

        def skip_func(self):
            return unittest.skip("Skipping draft test %s" % test_func.__name__)

        return skip_func
    return test_func


class gitutiltest(unittest.TestCase):
    """Unit tests for the gitutil helper class of the repo converter"""

    def test_parsedifftree_readable(self):
        difftree_string = """1fd915b9c1fcf3803383432ede29fc4d686fdb44
:100644 100644 b02a70992e734985768e839281932c315fafb21d a268f9d1a620a9f438d014376f72bcf413eea6d8 M\tlibc/arch-arm/bionic/__bionic_clone.S
:100644 100644 56ac0f69d450d218174226e1d61863a1ce5d4f27 27e44e7f7598ee1d2ca13df305f269d8ce303bfb M\tlibc/arch-arm64/bionic/__bionic_clone.S
1fd915b9c1fcf3803383432ede29fc4d686fdb44
:100644 100644 6439e31ce8bd4e4f1e290c3d8607cb7694a52b31 a8da3ac753c77d4a26f95bffcca546cc8cfb4f77 M\tlibc/dns/resolv/res_send.c
"""
        out = gitutil.parsedifftree(difftree_string)
        expected = [
            [
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "b02a70992e734985768e839281932c315fafb21d",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a268f9d1a620a9f438d014376f72bcf413eea6d8",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "56ac0f69d450d218174226e1d61863a1ce5d4f27",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "27e44e7f7598ee1d2ca13df305f269d8ce303bfb",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
            ],
            [
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "6439e31ce8bd4e4f1e290c3d8607cb7694a52b31",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a8da3ac753c77d4a26f95bffcca546cc8cfb4f77",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "status": "M",
                    "score": None,
                }
            ],
        ]
        self.assertEqual(out, expected)

    def test_parsedifftree_compact(self):
        difftree_string = """1fd915b9c1fcf3803383432ede29fc4d686fdb44\x00:100644 100644 b02a70992e734985768e839281932c315fafb21d a268f9d1a620a9f438d014376f72bcf413eea6d8 M\x00libc/arch-arm/bionic/__bionic_clone.S\x00:100644 100644 56ac0f69d450d218174226e1d61863a1ce5d4f27 27e44e7f7598ee1d2ca13df305f269d8ce303bfb M\x00libc/arch-arm64/bionic/__bionic_clone.S\x001fd915b9c1fcf3803383432ede29fc4d686fdb44\x00:100644 100644 6439e31ce8bd4e4f1e290c3d8607cb7694a52b31 a8da3ac753c77d4a26f95bffcca546cc8cfb4f77 M\x00libc/dns/resolv/res_send.c\x00"""
        out = gitutil.parsedifftree(difftree_string)
        expected = [
            [  # Parent 1
                {  # File 1
                    "source": {
                        "mode": 0o100644,
                        "hash": "b02a70992e734985768e839281932c315fafb21d",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a268f9d1a620a9f438d014376f72bcf413eea6d8",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
                {  # File 2
                    "source": {
                        "mode": 0o100644,
                        "hash": "56ac0f69d450d218174226e1d61863a1ce5d4f27",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "27e44e7f7598ee1d2ca13df305f269d8ce303bfb",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
            ],
            [  # Parent 2
                {  # File 1
                    "source": {
                        "mode": 0o100644,
                        "hash": "6439e31ce8bd4e4f1e290c3d8607cb7694a52b31",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a8da3ac753c77d4a26f95bffcca546cc8cfb4f77",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "status": "M",
                    "score": None,
                }
            ],
        ]
        self.assertEqual(out, expected)

    def test_parsegitcommitraw(self):
        commit_hash = "6c6677a7b5cf683a1883bc5e4ad47cad0a496904"
        commit_string = """tree e2acedaa094c4b5f0606e2a5ff58c3648555cfd4
parent c6c89b3401f3f6690e2307de7e2d079894c8147a
parent 2051d0428d045796ded3764c4188249669d1fcf3
author Linux Build Service Account <lnxbuild@localhost> 1521780995 -0700
committer Linux Build Service Account <lnxbuild@localhost> 1521780995 -0700

Merge AU_LINUX_ANDROID_LA.BR.1.3.7_RB1.08.01.00.336.038 on remote branch

Change-Id: Ie8ded3a8316b465c89a256c1a9146345614ed68f"""
        out = gitutil.parsegitcommitraw(commit_hash, commit_string)

        self.assertEqual(out.rev, "6c6677a7b5cf683a1883bc5e4ad47cad0a496904")
        self.assertEqual(
            out.desc,
            """Merge AU_LINUX_ANDROID_LA.BR.1.3.7_RB1.08.01.00.336.038 on remote branch

Change-Id: Ie8ded3a8316b465c89a256c1a9146345614ed68f""",
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
