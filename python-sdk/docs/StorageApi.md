# qarax.StorageApi

All URIs are relative to */*

Method | HTTP request | Description
------------- | ------------- | -------------
[**storage_get**](StorageApi.md#storage_get) | **GET** /storage/ | get storages list
[**storage_post**](StorageApi.md#storage_post) | **POST** /storage/ | Add new storage

# **storage_get**
> list[Storage] storage_get()

get storages list

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.StorageApi()

try:
    # get storages list
    api_response = api_instance.storage_get()
    pprint(api_response)
except ApiException as e:
    print("Exception when calling StorageApi->storage_get: %s\n" % e)
```

### Parameters
This endpoint does not need any parameter.

### Return type

[**list[Storage]**](Storage.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **storage_post**
> PostResponse storage_post(body=body)

Add new storage

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.StorageApi()
body = qarax.Storage() # Storage |  (optional)

try:
    # Add new storage
    api_response = api_instance.storage_post(body=body)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling StorageApi->storage_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **body** | [**Storage**](Storage.md)|  | [optional] 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

