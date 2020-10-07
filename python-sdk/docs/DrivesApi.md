# qarax.DrivesApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_drive**](DrivesApi.md#add_drive) | **POST** /drives/ | Add new drive
[**list_drives**](DrivesApi.md#list_drives) | **GET** /drives/ | Get drives list


# **add_drive**
> PostResponse add_drive(drive=drive)

Add new drive

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
    api_instance = qarax.DrivesApi(api_client)
    drive = qarax.Drive() # Drive |  (optional)

    try:
        # Add new drive
        api_response = api_instance.add_drive(drive=drive)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling DrivesApi->add_drive: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **drive** | [**Drive**](Drive.md)|  | [optional] 

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

# **list_drives**
> list[Drive] list_drives()

Get drives list

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
    api_instance = qarax.DrivesApi(api_client)
    
    try:
        # Get drives list
        api_response = api_instance.list_drives()
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling DrivesApi->list_drives: %s\n" % e)
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

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | successful operation |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

