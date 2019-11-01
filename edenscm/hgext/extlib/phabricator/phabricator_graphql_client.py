# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, unicode_literals

import time


class PhabricatorGraphQLClient(object):
    def __init__(
        self,
        urllib,
        ph_cert,
        ph_oauth,
        ph_cats,
        ph_user_name,
        source,
        host,
        app_id,
        app_token,
        ca_bundle=None,
    ):
        self.urllib = urllib
        self.phabricator_certificate = ph_cert
        self.phabricator_oauth = ph_oauth
        self.phabricator_cats = ph_cats
        self.user = ph_user_name
        self.__cert_time = 0
        self.graphql_url = host + "/graphql"
        self.token_url = host + "/phabricator/get_token"
        self.source = source
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
            self._checkconnection(timeout)
            data = {
                "phabricator_token": self.__cert,
                "doc": request,
                "variables": params,
            }

        return self.urllib.sendpost(
            self.graphql_url, data=data, timeout=timeout, ca_bundle=self.ca_bundle
        )

    def _checkconnection(self, timeout):
        """
        We only care about the expiring Phabricator token if we don't have
        an OAuth token
        """
        if self.phabricator_oauth is None:
            if time.time() - self.__cert_time > 600:
                self._connect(timeout)
            data = {"phabricator_token": self.__cert, "source": self.source}
            self.urllib.sendpost(
                self.graphql_url, data=data, timeout=timeout, ca_bundle=self.ca_bundle
            )

    def _connect(self, timeout):
        """
        Private method to get token to make calls, unless we're using the
        OAuth token, then we just do nothing here
        """
        if self.phabricator_oauth is None:
            data = {
                "ph_certificate": self.phabricator_certificate,
                "ph_user_name": self.user,
                "app": self.app_id,
                "token": self.app_token,
            }
            res = self.urllib.sendpost(
                self.token_url, data=data, timeout=timeout, ca_bundle=self.ca_bundle
            )
            self.__cert = res.get("token")
        self.__cert_time = time.time()
