# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, unicode_literals

from . import arcconfig


class PhabricatorGraphQLClient(object):
    def __init__(
        self, urllib, ph_oauth, ph_cats, host, app_id, app_token, ca_bundle=None
    ):
        self.urllib = urllib
        self.phabricator_oauth = ph_oauth
        self.phabricator_cats = ph_cats
        self.graphql_url = host + "/graphql"
        self.app_id = app_id
        self.app_token = app_token
        self.ca_bundle = ca_bundle

    def query(self, timeout, request, params=None):
        """
        Make a graphql2 (OSS) request to phabricator data
        """
        if self.phabricator_oauth is not None:
            data = {
                "access_token": self.phabricator_oauth,
                "doc": request,
                "variables": params,
            }
        elif self.phabricator_cats is not None:
            data = {
                "crypto_auth_tokens": self.phabricator_cats,
                "cat_app": 197058370321847,
                "doc": request,
                "variables": params,
            }
        else:
            raise arcconfig.ArcConfigError(
                "The arcrc was missing valid authentication (either OAuth or CATs). "
                "For humans, follow the instructions at "
                "https://www.internalfb.com/intern/jf/authenticate/ "
                "to get a Phabricator OAuth token. "
                "For bots, see http://fburl.com/botdiffs for more info."
            )

        return self.urllib.sendpost(
            self.graphql_url, data=data, timeout=timeout, ca_bundle=self.ca_bundle
        )
