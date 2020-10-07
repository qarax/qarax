# qarax.KernelsApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_kernel**](KernelsApi.md#add_kernel) | **POST** /kernels/ | Add new kernel
[**get_kernel_storage**](KernelsApi.md#get_kernel_storage) | **GET** /kernels/{kernelId}/storage | 
[**list_kernel**](KernelsApi.md#list_kernel) | **GET** /kernels/ | get kernels list


# **add_kernel**
> PostResponse add_kernel(kernel=kernel)

Add new kernel

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
    api_instance = qarax.KernelsApi(api_client)
    kernel = qarax.Kernel() # Kernel |  (optional)

    try:
        # Add new kernel
        api_response = api_instance.add_kernel(kernel=kernel)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling KernelsApi->add_kernel: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **kernel** | [**Kernel**](Kernel.md)|  | [optional] 

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

# **get_kernel_storage**
> Storage get_kernel_storage(kernel_id)



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
    api_instance = qarax.KernelsApi(api_client)
    kernel_id = 'kernel_id_example' # str | ID of a kernel

    try:
        api_response = api_instance.get_kernel_storage(kernel_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling KernelsApi->get_kernel_storage: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **kernel_id** | **str**| ID of a kernel | 

### Return type

[**Storage**](Storage.md)

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

# **list_kernel**
> list[Kernel] list_kernel()

get kernels list

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
    api_instance = qarax.KernelsApi(api_client)
    
    try:
        # get kernels list
        api_response = api_instance.list_kernel()
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling KernelsApi->list_kernel: %s\n" % e)
```

### Parameters
This endpoint does not need any parameter.

### Return type

[**list[Kernel]**](Kernel.md)

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

