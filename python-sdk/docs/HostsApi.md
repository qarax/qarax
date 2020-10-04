# qarax.HostsApi

All URIs are relative to */*

Method | HTTP request | Description
------------- | ------------- | -------------
[**hosts_get**](HostsApi.md#hosts_get) | **GET** /hosts | Get hosts list
[**hosts_host_id_get**](HostsApi.md#hosts_host_id_get) | **GET** /hosts/{hostId} | Get host by ID
[**hosts_host_id_health_get**](HostsApi.md#hosts_host_id_health_get) | **GET** /hosts/{hostId}/health | Host health check
[**hosts_host_id_install_post**](HostsApi.md#hosts_host_id_install_post) | **POST** /hosts/{hostId}/install | Install qarax node on host
[**hosts_post**](HostsApi.md#hosts_post) | **POST** /hosts | Create new host

# **hosts_get**
> list[Host] hosts_get()

Get hosts list

Get hosts list

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.HostsApi()

try:
    # Get hosts list
    api_response = api_instance.hosts_get()
    pprint(api_response)
except ApiException as e:
    print("Exception when calling HostsApi->hosts_get: %s\n" % e)
```

### Parameters
This endpoint does not need any parameter.

### Return type

[**list[Host]**](Host.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **hosts_host_id_get**
> Host hosts_host_id_get(host_id)

Get host by ID

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.HostsApi()
host_id = 'host_id_example' # str | ID of host

try:
    # Get host by ID
    api_response = api_instance.hosts_host_id_get(host_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling HostsApi->hosts_host_id_get: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **host_id** | **str**| ID of host | 

### Return type

[**Host**](Host.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **hosts_host_id_health_get**
> HealthResponse hosts_host_id_health_get(host_id)

Host health check

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.HostsApi()
host_id = 'host_id_example' # str | ID of host

try:
    # Host health check
    api_response = api_instance.hosts_host_id_health_get(host_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling HostsApi->hosts_host_id_health_get: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **host_id** | **str**| ID of host | 

### Return type

[**HealthResponse**](HealthResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **hosts_host_id_install_post**
> list[InstallHost] hosts_host_id_install_post(host_id, body=body)

Install qarax node on host

Install and run qarax-node on host

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.HostsApi()
host_id = 'host_id_example' # str | ID of host
body = qarax.InstallHost() # InstallHost |  (optional)

try:
    # Install qarax node on host
    api_response = api_instance.hosts_host_id_install_post(host_id, body=body)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling HostsApi->hosts_host_id_install_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **host_id** | **str**| ID of host | 
 **body** | [**InstallHost**](InstallHost.md)|  | [optional] 

### Return type

[**list[InstallHost]**](InstallHost.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **hosts_post**
> PostResponse hosts_post(body)

Create new host

Create new host

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.HostsApi()
body = qarax.Host() # Host | 

try:
    # Create new host
    api_response = api_instance.hosts_post(body)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling HostsApi->hosts_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **body** | [**Host**](Host.md)|  | 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

