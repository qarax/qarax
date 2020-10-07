# qarax.VMsApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_vm**](VMsApi.md#add_vm) | **POST** /vms/ | Add new VM
[**attach_drive**](VMsApi.md#attach_drive) | **POST** /vms/{vmId}/drives/{driveId}/attach | Add drive to VM
[**get_vm**](VMsApi.md#get_vm) | **GET** /vms/{vmId}/ | VM details
[**list_vm_drives**](VMsApi.md#list_vm_drives) | **GET** /vms/{vmId}/drives/ | 
[**list_vms**](VMsApi.md#list_vms) | **GET** /vms/ | get vms list
[**start_vm**](VMsApi.md#start_vm) | **POST** /vms/{vmId}/start | Start VM
[**stop_vm**](VMsApi.md#stop_vm) | **POST** /vms/{vmId}/stop | Stop VM


# **add_vm**
> PostResponse add_vm(vm=vm)

Add new VM

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
    api_instance = qarax.VMsApi(api_client)
    vm = qarax.Vm() # Vm |  (optional)

    try:
        # Add new VM
        api_response = api_instance.add_vm(vm=vm)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->add_vm: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm** | [**Vm**](Vm.md)|  | [optional] 

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

# **attach_drive**
> list[AttachDrive] attach_drive(vm_id, drive_id)

Add drive to VM

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
    api_instance = qarax.VMsApi(api_client)
    vm_id = 'vm_id_example' # str | ID of a VM
drive_id = 'drive_id_example' # str | ID of a drive

    try:
        # Add drive to VM
        api_response = api_instance.attach_drive(vm_id, drive_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->attach_drive: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm_id** | **str**| ID of a VM | 
 **drive_id** | **str**| ID of a drive | 

### Return type

[**list[AttachDrive]**](AttachDrive.md)

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

# **get_vm**
> Vm get_vm(vm_id)

VM details

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
    api_instance = qarax.VMsApi(api_client)
    vm_id = 'vm_id_example' # str | ID of a VM

    try:
        # VM details
        api_response = api_instance.get_vm(vm_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->get_vm: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm_id** | **str**| ID of a VM | 

### Return type

[**Vm**](Vm.md)

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

# **list_vm_drives**
> list[Drive] list_vm_drives(vm_id)



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
    api_instance = qarax.VMsApi(api_client)
    vm_id = 'vm_id_example' # str | ID of a VM

    try:
        api_response = api_instance.list_vm_drives(vm_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->list_vm_drives: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm_id** | **str**| ID of a VM | 

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

# **list_vms**
> list[Vm] list_vms()

get vms list

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
    api_instance = qarax.VMsApi(api_client)
    
    try:
        # get vms list
        api_response = api_instance.list_vms()
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->list_vms: %s\n" % e)
```

### Parameters
This endpoint does not need any parameter.

### Return type

[**list[Vm]**](Vm.md)

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

# **start_vm**
> PostResponse start_vm(vm_id)

Start VM

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
    api_instance = qarax.VMsApi(api_client)
    vm_id = 'vm_id_example' # str | ID of a VM

    try:
        # Start VM
        api_response = api_instance.start_vm(vm_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->start_vm: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm_id** | **str**| ID of a VM | 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | host status |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **stop_vm**
> PostResponse stop_vm(vm_id)

Stop VM

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
    api_instance = qarax.VMsApi(api_client)
    vm_id = 'vm_id_example' # str | ID of a VM

    try:
        # Stop VM
        api_response = api_instance.stop_vm(vm_id)
        pprint(api_response)
    except ApiException as e:
        print("Exception when calling VMsApi->stop_vm: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **vm_id** | **str**| ID of a VM | 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | host status |  -  |
**0** | unexpected error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

