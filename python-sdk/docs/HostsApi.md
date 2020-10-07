# qarax.HostsApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_host**](HostsApi.md#add_host) | **POST** /hosts | Create new host
[**get_host**](HostsApi.md#get_host) | **GET** /hosts/{hostId} | Get host by ID
[**health_check**](HostsApi.md#health_check) | **GET** /hosts/{hostId}/health | Host health check
[**install_host**](HostsApi.md#install_host) | **POST** /hosts/{hostId}/install | Install qarax node on host
[**list_hosts**](HostsApi.md#list_hosts) | **GET** /hosts | Get hosts list


# **add_host**
> PostResponse add_host(host)

Create new host

Create new host

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
    api_instance = qarax.HostsApi(api_client)
    host = qarax.Host() # Host | 

    try:
        # Create new host
        api_response = api_instance.add_host(host)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling HostsApi->add_host: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **host** | [**Host**](Host.md)|  | 

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

# **get_host**
> Host get_host(host_id)

Get host by ID

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
    api_instance = qarax.HostsApi(api_client)
    host_id = 'host_id_example' # str | ID of host

    try:
        # Get host by ID
        api_response = api_instance.get_host(host_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling HostsApi->get_host: %s\n" % e)
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

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | successful operation |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **health_check**
> HealthResponse health_check(host_id)

Host health check

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
    api_instance = qarax.HostsApi(api_client)
    host_id = 'host_id_example' # str | ID of host

    try:
        # Host health check
        api_response = api_instance.health_check(host_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling HostsApi->health_check: %s\n" % e)
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

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**0** | Host health check result |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **install_host**
> InstallHost install_host(host_id, install_host=install_host)

Install qarax node on host

Install and run qarax-node on host

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
    api_instance = qarax.HostsApi(api_client)
    host_id = 'host_id_example' # str | ID of host
install_host = qarax.InstallHost() # InstallHost |  (optional)

    try:
        # Install qarax node on host
        api_response = api_instance.install_host(host_id, install_host=install_host)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling HostsApi->install_host: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **host_id** | **str**| ID of host | 
 **install_host** | [**InstallHost**](InstallHost.md)|  | [optional] 

### Return type

[**InstallHost**](InstallHost.md)

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

# **list_hosts**
> list[Host] list_hosts()

Get hosts list

Get hosts list

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
    api_instance = qarax.HostsApi(api_client)
    
    try:
        # Get hosts list
        api_response = api_instance.list_hosts()
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling HostsApi->list_hosts: %s\n" % e)
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

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | successful operation |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

