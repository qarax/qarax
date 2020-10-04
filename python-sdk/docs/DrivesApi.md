# qarax.DrivesApi

All URIs are relative to */*

Method | HTTP request | Description
------------- | ------------- | -------------
[**drives_get**](DrivesApi.md#drives_get) | **GET** /drives/ | Get drives list
[**drives_post**](DrivesApi.md#drives_post) | **POST** /drives/ | Add new drive

# **drives_get**
> list[Drive] drives_get()

Get drives list

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.DrivesApi()

try:
    # Get drives list
    api_response = api_instance.drives_get()
    pprint(api_response)
except ApiException as e:
    print("Exception when calling DrivesApi->drives_get: %s\n" % e)
```

### Parameters
This endpoint does not need any parameter.

### Return type

[**list[Drive]**](Drive.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **drives_post**
> PostResponse drives_post(body=body)

Add new drive

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.DrivesApi()
body = qarax.Drive() # Drive |  (optional)

try:
    # Add new drive
    api_response = api_instance.drives_post(body=body)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling DrivesApi->drives_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **body** | [**Drive**](Drive.md)|  | [optional] 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

