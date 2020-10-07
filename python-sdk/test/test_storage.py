# coding: utf-8

"""
    Qarax API

    The API for Qarax manager  # noqa: E501

    The version of the OpenAPI document: 0.1.0
    Generated by: https://openapi-generator.tech
"""


from __future__ import absolute_import

import unittest
import datetime

import qarax
from qarax.models.storage import Storage  # noqa: E501
from qarax.rest import ApiException

class TestStorage(unittest.TestCase):
    """Storage unit test stubs"""

    def setUp(self):
        pass

    def tearDown(self):
        pass

    def make_instance(self, include_optional):
        """Test Storage
            include_option is a boolean, when False only required
            params are included, when True both required and
            optional params are included """
        # model = qarax.models.storage.Storage()  # noqa: E501
        if include_optional :
            return Storage(
                config = None, 
                id = '0', 
                name = '0', 
                status = '0', 
                storage_type = '0'
            )
        else :
            return Storage(
        )

    def testStorage(self):
        """Test Storage"""
        inst_req_only = self.make_instance(include_optional=False)
        inst_req_and_optional = self.make_instance(include_optional=True)


if __name__ == '__main__':
    unittest.main()
