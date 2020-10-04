# coding: utf-8

"""
    Qarax API

    The API for Qarax manager  # noqa: E501

    OpenAPI spec version: 0.1.0
    
    Generated by: https://github.com/swagger-api/swagger-codegen.git
"""

from __future__ import absolute_import

import re  # noqa: F401

# python 2 and python 3 compatibility library
import six

from qarax.api_client import ApiClient


class KernelsApi(object):
    """NOTE: This class is auto generated by the swagger code generator program.

    Do not edit the class manually.
    Ref: https://github.com/swagger-api/swagger-codegen
    """

    def __init__(self, api_client=None):
        if api_client is None:
            api_client = ApiClient()
        self.api_client = api_client

    def kernels_get(self, **kwargs):  # noqa: E501
        """get kernels list  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_get(async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :return: list[Kernel]
                 If the method is called asynchronously,
                 returns the request thread.
        """
        kwargs['_return_http_data_only'] = True
        if kwargs.get('async_req'):
            return self.kernels_get_with_http_info(**kwargs)  # noqa: E501
        else:
            (data) = self.kernels_get_with_http_info(**kwargs)  # noqa: E501
            return data

    def kernels_get_with_http_info(self, **kwargs):  # noqa: E501
        """get kernels list  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_get_with_http_info(async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :return: list[Kernel]
                 If the method is called asynchronously,
                 returns the request thread.
        """

        all_params = []  # noqa: E501
        all_params.append('async_req')
        all_params.append('_return_http_data_only')
        all_params.append('_preload_content')
        all_params.append('_request_timeout')

        params = locals()
        for key, val in six.iteritems(params['kwargs']):
            if key not in all_params:
                raise TypeError(
                    "Got an unexpected keyword argument '%s'"
                    " to method kernels_get" % key
                )
            params[key] = val
        del params['kwargs']

        collection_formats = {}

        path_params = {}

        query_params = []

        header_params = {}

        form_params = []
        local_var_files = {}

        body_params = None
        # HTTP header `Accept`
        header_params['Accept'] = self.api_client.select_header_accept(
            ['application/json'])  # noqa: E501

        # Authentication setting
        auth_settings = []  # noqa: E501

        return self.api_client.call_api(
            '/kernels/', 'GET',
            path_params,
            query_params,
            header_params,
            body=body_params,
            post_params=form_params,
            files=local_var_files,
            response_type='list[Kernel]',  # noqa: E501
            auth_settings=auth_settings,
            async_req=params.get('async_req'),
            _return_http_data_only=params.get('_return_http_data_only'),
            _preload_content=params.get('_preload_content', True),
            _request_timeout=params.get('_request_timeout'),
            collection_formats=collection_formats)

    def kernels_kernel_id_storage_get(self, kernel_id, **kwargs):  # noqa: E501
        """kernels_kernel_id_storage_get  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_kernel_id_storage_get(kernel_id, async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :param str kernel_id: ID of a kernel (required)
        :return: Storage
                 If the method is called asynchronously,
                 returns the request thread.
        """
        kwargs['_return_http_data_only'] = True
        if kwargs.get('async_req'):
            return self.kernels_kernel_id_storage_get_with_http_info(kernel_id, **kwargs)  # noqa: E501
        else:
            (data) = self.kernels_kernel_id_storage_get_with_http_info(kernel_id, **kwargs)  # noqa: E501
            return data

    def kernels_kernel_id_storage_get_with_http_info(self, kernel_id, **kwargs):  # noqa: E501
        """kernels_kernel_id_storage_get  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_kernel_id_storage_get_with_http_info(kernel_id, async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :param str kernel_id: ID of a kernel (required)
        :return: Storage
                 If the method is called asynchronously,
                 returns the request thread.
        """

        all_params = ['kernel_id']  # noqa: E501
        all_params.append('async_req')
        all_params.append('_return_http_data_only')
        all_params.append('_preload_content')
        all_params.append('_request_timeout')

        params = locals()
        for key, val in six.iteritems(params['kwargs']):
            if key not in all_params:
                raise TypeError(
                    "Got an unexpected keyword argument '%s'"
                    " to method kernels_kernel_id_storage_get" % key
                )
            params[key] = val
        del params['kwargs']
        # verify the required parameter 'kernel_id' is set
        if ('kernel_id' not in params or
                params['kernel_id'] is None):
            raise ValueError("Missing the required parameter `kernel_id` when calling `kernels_kernel_id_storage_get`")  # noqa: E501

        collection_formats = {}

        path_params = {}
        if 'kernel_id' in params:
            path_params['kernelId'] = params['kernel_id']  # noqa: E501

        query_params = []

        header_params = {}

        form_params = []
        local_var_files = {}

        body_params = None
        # HTTP header `Accept`
        header_params['Accept'] = self.api_client.select_header_accept(
            ['application/json'])  # noqa: E501

        # Authentication setting
        auth_settings = []  # noqa: E501

        return self.api_client.call_api(
            '/kernels/{kernelId}/storage', 'GET',
            path_params,
            query_params,
            header_params,
            body=body_params,
            post_params=form_params,
            files=local_var_files,
            response_type='Storage',  # noqa: E501
            auth_settings=auth_settings,
            async_req=params.get('async_req'),
            _return_http_data_only=params.get('_return_http_data_only'),
            _preload_content=params.get('_preload_content', True),
            _request_timeout=params.get('_request_timeout'),
            collection_formats=collection_formats)

    def kernels_post(self, **kwargs):  # noqa: E501
        """Add new kernel  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_post(async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :param Kernel body:
        :return: PostResponse
                 If the method is called asynchronously,
                 returns the request thread.
        """
        kwargs['_return_http_data_only'] = True
        if kwargs.get('async_req'):
            return self.kernels_post_with_http_info(**kwargs)  # noqa: E501
        else:
            (data) = self.kernels_post_with_http_info(**kwargs)  # noqa: E501
            return data

    def kernels_post_with_http_info(self, **kwargs):  # noqa: E501
        """Add new kernel  # noqa: E501

        This method makes a synchronous HTTP request by default. To make an
        asynchronous HTTP request, please pass async_req=True
        >>> thread = api.kernels_post_with_http_info(async_req=True)
        >>> result = thread.get()

        :param async_req bool
        :param Kernel body:
        :return: PostResponse
                 If the method is called asynchronously,
                 returns the request thread.
        """

        all_params = ['body']  # noqa: E501
        all_params.append('async_req')
        all_params.append('_return_http_data_only')
        all_params.append('_preload_content')
        all_params.append('_request_timeout')

        params = locals()
        for key, val in six.iteritems(params['kwargs']):
            if key not in all_params:
                raise TypeError(
                    "Got an unexpected keyword argument '%s'"
                    " to method kernels_post" % key
                )
            params[key] = val
        del params['kwargs']

        collection_formats = {}

        path_params = {}

        query_params = []

        header_params = {}

        form_params = []
        local_var_files = {}

        body_params = None
        if 'body' in params:
            body_params = params['body']
        # HTTP header `Accept`
        header_params['Accept'] = self.api_client.select_header_accept(
            ['application/json'])  # noqa: E501

        # HTTP header `Content-Type`
        header_params['Content-Type'] = self.api_client.select_header_content_type(  # noqa: E501
            ['application/json'])  # noqa: E501

        # Authentication setting
        auth_settings = []  # noqa: E501

        return self.api_client.call_api(
            '/kernels/', 'POST',
            path_params,
            query_params,
            header_params,
            body=body_params,
            post_params=form_params,
            files=local_var_files,
            response_type='PostResponse',  # noqa: E501
            auth_settings=auth_settings,
            async_req=params.get('async_req'),
            _return_http_data_only=params.get('_return_http_data_only'),
            _preload_content=params.get('_preload_content', True),
            _request_timeout=params.get('_request_timeout'),
            collection_formats=collection_formats)
