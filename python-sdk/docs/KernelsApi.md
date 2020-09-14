# qarax.KernelsApi

All URIs are relative to */*

Method | HTTP request | Description
------------- | ------------- | -------------
[**kernels_get**](KernelsApi.md#kernels_get) | **GET** /kernels/ | get kernels list
[**kernels_kernel_id_storage_get**](KernelsApi.md#kernels_kernel_id_storage_get) | **GET** /kernels/{kernelId}/storage | 
[**kernels_post**](KernelsApi.md#kernels_post) | **POST** /kernels/ | Add new kernel

# **kernels_get**
> list[Kernel] kernels_get()

get kernels list

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.KernelsApi()

try:
    # get kernels list
    api_response = api_instance.kernels_get()
    pprint(api_response)
except ApiException as e:
    print("Exception when calling KernelsApi->kernels_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **kernels_kernel_id_storage_get**
> Storage kernels_kernel_id_storage_get(kernel_id)



### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.KernelsApi()
kernel_id = 'kernel_id_example' # str | ID of a kernel

try:
    api_response = api_instance.kernels_kernel_id_storage_get(kernel_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling KernelsApi->kernels_kernel_id_storage_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **kernels_post**
> kernels_post(body=body)

Add new kernel

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.KernelsApi()
body = qarax.Kernel() # Kernel |  (optional)

try:
    # Add new kernel
    api_instance.kernels_post(body=body)
except ApiException as e:
    print("Exception when calling KernelsApi->kernels_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **body** | [**Kernel**](Kernel.md)|  | [optional] 

### Return type

void (empty response body)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

