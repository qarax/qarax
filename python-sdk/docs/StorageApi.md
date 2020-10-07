# qarax.StorageApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_storage**](StorageApi.md#add_storage) | **POST** /storage/ | Add new storage
[**list_storage**](StorageApi.md#list_storage) | **GET** /storage/ | get storages list


# **add_storage**
> PostResponse add_storage(storage=storage)

Add new storage

### Example

```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint
# Defining the host is optional and defaults to http://localhost
# See configuration.py for a list of all supported configuration parameters.
configuration = qarax.Configuration(
    host = "http://localhost"
)


# Enter a context with an instance of the API client
with qarax.ApiClient() as api_client:
    # Create an instance of the API class
    api_instance = qarax.StorageApi(api_client)
    storage = qarax.Storage() # Storage |  (optional)

    try:
        # Add new storage
        api_response = api_instance.add_storage(storage=storage)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling StorageApi->add_storage: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **storage** | [**Storage**](Storage.md)|  | [optional] 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | successful operation |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_storage**
> list[Storage] list_storage()

get storages list

### Example

```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint
# Defining the host is optional and defaults to http://localhost
# See configuration.py for a list of all supported configuration parameters.
configuration = qarax.Configuration(
    host = "http://localhost"
)


# Enter a context with an instance of the API client
with qarax.ApiClient() as api_client:
    # Create an instance of the API class
    api_instance = qarax.StorageApi(api_client)
    
    try:
        # get storages list
        api_response = api_instance.list_storage()
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling StorageApi->list_storage: %s\n" % e)
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

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | successful operation |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

