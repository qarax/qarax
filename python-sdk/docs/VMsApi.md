# qarax.VMsApi

All URIs are relative to */*

Method | HTTP request | Description
------------- | ------------- | -------------
[**vms_get**](VMsApi.md#vms_get) | **GET** /vms/ | get vms list
[**vms_post**](VMsApi.md#vms_post) | **POST** /vms/ | Add new VM
[**vms_vm_id_drives_drive_id_attach_post**](VMsApi.md#vms_vm_id_drives_drive_id_attach_post) | **POST** /vms/{vmId}/drives/{driveId}/attach | Add drive to VM
[**vms_vm_id_drives_get**](VMsApi.md#vms_vm_id_drives_get) | **GET** /vms/{vmId}/drives/ | 
[**vms_vm_id_get**](VMsApi.md#vms_vm_id_get) | **GET** /vms/{vmId}/ | VM details
[**vms_vm_id_start_post**](VMsApi.md#vms_vm_id_start_post) | **POST** /vms/{vmId}/start | Start VM
[**vms_vm_id_stop_post**](VMsApi.md#vms_vm_id_stop_post) | **POST** /vms/{vmId}/stop | Stop VM

# **vms_get**
> list[Vm] vms_get()

get vms list

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()

try:
    # get vms list
    api_response = api_instance.vms_get()
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_post**
> PostResponse vms_post(body=body)

Add new VM

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
body = qarax.Vm() # Vm |  (optional)

try:
    # Add new VM
    api_response = api_instance.vms_post(body=body)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_post: %s\n" % e)
```

### Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **body** | [**Vm**](Vm.md)|  | [optional] 

### Return type

[**PostResponse**](PostResponse.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_vm_id_drives_drive_id_attach_post**
> list[AttachDrive] vms_vm_id_drives_drive_id_attach_post(vm_id, drive_id)

Add drive to VM

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
vm_id = 'vm_id_example' # str | ID of a VM
drive_id = 'drive_id_example' # str | ID of a drive

try:
    # Add drive to VM
    api_response = api_instance.vms_vm_id_drives_drive_id_attach_post(vm_id, drive_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_vm_id_drives_drive_id_attach_post: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_vm_id_drives_get**
> list[Drive] vms_vm_id_drives_get(vm_id)



### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
vm_id = 'vm_id_example' # str | ID of a VM

try:
    api_response = api_instance.vms_vm_id_drives_get(vm_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_vm_id_drives_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_vm_id_get**
> Vm vms_vm_id_get(vm_id)

VM details

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
vm_id = 'vm_id_example' # str | ID of a VM

try:
    # VM details
    api_response = api_instance.vms_vm_id_get(vm_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_vm_id_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_vm_id_start_post**
> PostResponse vms_vm_id_start_post(vm_id)

Start VM

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
vm_id = 'vm_id_example' # str | ID of a VM

try:
    # Start VM
    api_response = api_instance.vms_vm_id_start_post(vm_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_vm_id_start_post: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **vms_vm_id_stop_post**
> PostResponse vms_vm_id_stop_post(vm_id)

Stop VM

### Example
```python
from __future__ import print_function
import time
import qarax
from qarax.rest import ApiException
from pprint import pprint

# create an instance of the API class
api_instance = qarax.VMsApi()
vm_id = 'vm_id_example' # str | ID of a VM

try:
    # Stop VM
    api_response = api_instance.vms_vm_id_stop_post(vm_id)
    pprint(api_response)
except ApiException as e:
    print("Exception when calling VMsApi->vms_vm_id_stop_post: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

