/*
MIT License

Copyright (c) 2025 Open Device Partnership

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

#define INITGUID
#include <windows.h>
#include <strsafe.h>
#include <cfgmgr32.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <SetupAPI.h>
#include <Devpkey.h>
#include <Acpiioct.h>
#include <devioctl.h>
#include "..\inc\eclib.h"
#include "..\inc\ectest.h"

#define MAX_DEVPATH_LENGTH  64

// GUID defined in the KMDF INX file for ectest.sys
// {5362ad97-ddfe-429d-9305-31c0ad27880a}
const GUID GUID_DEVCLASS_ECTEST = { 0x5362ad97, 0xddfe, 0x429d, { 0x93, 0x05, 0x31, 0xc0, 0xad, 0x27, 0x88, 0x0a } };

typedef struct {
    CRITICAL_SECTION lock;
    CONDITION_VARIABLE cv;
    BOOL in_progress;
    BOOL initialized;
    UINT32 event;
    HANDLE handle;
} NotificationState;

static NotificationState g_notify;

/*
 * Function: GetGUIDPath
 * ---------------------
 * Retrieves the device path for a device matching the specified device class GUID and device name.
 *
 * Parameters:
 *   GUID GUID_DEVCLASS_SYSTEM - The GUID of the device class to search for.
 *   const wchar_t* name       - The name of the device to match.
 *   wchar_t* path             - Output buffer for the device path.
 *   size_t path_len           - Length of the output buffer.
 *
 * Returns:
 *   wchar_t* - Pointer to the device path if found, NULL otherwise.
 */
wchar_t *GetGUIDPath(
    _In_ GUID GUID_DEVCLASS_SYSTEM,
    _In_ const wchar_t* name,
    _Out_ wchar_t* path,
    _In_ size_t path_len
)
{
    // Get devices of ACPI class there should only be one on the system
    BYTE PropertyBuffer[128];

    HDEVINFO DeviceInfoSet = SetupDiGetClassDevs(&GUID_DEVCLASS_SYSTEM, NULL, NULL, DIGCF_PRESENT);
    SP_DEVINFO_DATA DeviceInfoData = { .cbSize = sizeof(SP_DEVINFO_DATA) };
    DWORD DeviceIndex = 0;
    BOOL bRet = TRUE;
    BOOL bPathFound = FALSE;

    while (SetupDiEnumDeviceInfo(DeviceInfoSet, DeviceIndex, &DeviceInfoData)) 
    {
        // Read Device instance path and check for ACPI_HAL\PNP0C08 as this is the ACPI driver
        DEVPROPTYPE PropertyType;
        DWORD RequiredSize = 0;
        bRet = SetupDiGetDevicePropertyW(
            DeviceInfoSet,
            &DeviceInfoData,
            &DEVPKEY_Device_InstanceId,
            &PropertyType,
            PropertyBuffer,
            sizeof(PropertyBuffer),
            &RequiredSize,
            0);

        if (RequiredSize > 0 && wcsstr((wchar_t*)PropertyBuffer, name) ) {
            bRet = SetupDiGetDevicePropertyW(
                DeviceInfoSet,
                &DeviceInfoData,
                &DEVPKEY_Device_PDOName,
                &PropertyType,
                PropertyBuffer,
                sizeof(PropertyBuffer),
                &RequiredSize,
                0);

            StringCchPrintf(path, path_len, L"\\\\.\\GLOBALROOT%ls", (wchar_t*)PropertyBuffer);
            bPathFound = TRUE;
            break;
        }
        DeviceIndex++;
    }

    if (DeviceInfoSet) {
        SetupDiDestroyDeviceInfoList(DeviceInfoSet);
    }

    // If device path was not found return NULL
    return bPathFound ? path : NULL;

}

/*
 * Function: GetKMDFDriverHandle
 * ----------------------------
 * Retrieves a handle to the KMDF driver by searching for the device path using the specified
 * device class GUID and device name, then opens the device.
 *
 * Parameters:
 *   DWORD flags      - Flags to open the file handle with.
 *   HANDLE *hDevice  - Pointer to a handle that will receive the device handle.
 *
 * Returns:
 *   int - ERROR_SUCCESS if successful, ERROR_INVALID_HANDLE otherwise.
 */
ECLIB_API
int GetKMDFDriverHandle(
    _In_ DWORD flags,
    _Out_ HANDLE *hDevice
    )
{
    WCHAR pathbuf[MAX_DEVPATH_LENGTH];
    int status = ERROR_SUCCESS;
    wchar_t *devicePath = GetGUIDPath(GUID_DEVCLASS_ECTEST,L"ETST0001",pathbuf,sizeof(pathbuf));

    if ( devicePath == NULL )
    {
        return ERROR_INVALID_HANDLE;
    }

    *hDevice = CreateFile(devicePath,
                         GENERIC_READ|GENERIC_WRITE,
                         FILE_SHARE_READ | FILE_SHARE_WRITE,
                         NULL,
                         OPEN_EXISTING,
                         flags,
                         NULL );

    if (*hDevice == INVALID_HANDLE_VALUE) {
        status = ERROR_INVALID_HANDLE;
    }

    return status;
}

/*
 * Function: EvaluateAcpi
 * ----------------------
 * Evaluates an ACPI method on the specified device and returns the result.
 *
 * Parameters:
 *   void* acpi_input   - Pointer to ACPI_EVAL_INPUT_xxxx structure.
 *   size_t input_len   - Length of the input structure.
 *   BYTE* buffer       - Output buffer for the result.
 *   size_t* buf_len    - Input: size of buffer; Output: bytes returned.
 *
 * Returns:
 *   int - ERROR_SUCCESS on success, ERROR_INVALID_PARAMETER on failure.
 */
ECLIB_API
int EvaluateAcpi(
    _In_ void* acpi_input,
    _In_ size_t input_len,
    _Out_ BYTE* buffer,
    _In_ size_t* buf_len
)
{
    WCHAR pathbuf[MAX_DEVPATH_LENGTH];
    ULONG bytesReturned;

    // Look up handle to ACPI entry
    wchar_t* dpath = GetGUIDPath(GUID_DEVCLASS_ECTEST, L"ETST0001", pathbuf, sizeof(pathbuf));
    if (dpath == NULL) {
        return ERROR_INVALID_PARAMETER;
    }

    HANDLE hDevice = CreateFile(dpath,
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        NULL,
        OPEN_EXISTING,
        0,
        NULL);

    if (hDevice != INVALID_HANDLE_VALUE) {
        if( DeviceIoControl(hDevice,
            (DWORD)IOCTL_ACPI_EVAL_METHOD_EX,
            acpi_input,
            (DWORD)input_len,
            buffer,
            (DWORD)*buf_len,
            &bytesReturned,
            NULL) == TRUE ) 
        {
            *buf_len = bytesReturned;
            return ERROR_SUCCESS;
        }
    }

    return ERROR_INVALID_PARAMETER;
}

/*
 * Function: InitializeNotification
 * -------------------------------
 * Initializes the notification system by setting up synchronization primitives
 * (critical section and condition variable) and opening a handle to the KMDF driver.
 * This function must be called before using notification-related APIs.
 *
 * Returns:
 *   INT32 - ERROR_SUCCESS on success, or an error code on failure.
 */
ECLIB_API
INT32 InitializeNotification()
{
    if(g_notify.initialized) {
        return ERROR_SUCCESS;
    }

    // Initialize critical section for notification handling
    InitializeCriticalSection(&g_notify.lock);
    InitializeConditionVariable(&g_notify.cv);
    g_notify.in_progress = FALSE;
    g_notify.event = 0;

    int status = GetKMDFDriverHandle( 0, &g_notify.handle );
    if(status != ERROR_SUCCESS || g_notify.handle == INVALID_HANDLE_VALUE) {
        DeleteCriticalSection(&g_notify.lock);
        return status;
    }
    
    g_notify.initialized = TRUE;
    return ERROR_SUCCESS;
}

/*
 * Function: CleanupNotification
 * ----------------------------
 * Cleans up the notification system by canceling any pending I/O operations,
 * closing the KMDF driver handle, and deleting the critical section.
 * Should be called when notification handling is no longer needed.
 *
 * Returns:
 *   VOID
 */
ECLIB_API
VOID CleanupNotification()
{
    if(!g_notify.initialized) {
        return;
    }
    
    EnterCriticalSection(&g_notify.lock);
    // Cancel any pending IO
    if(g_notify.handle) {
        while(g_notify.in_progress) {
            CancelIoEx(g_notify.handle, NULL);
            SleepConditionVariableCS(&g_notify.cv, &g_notify.lock, INFINITE);
        }
        CloseHandle(g_notify.handle);
        g_notify.handle = INVALID_HANDLE_VALUE;
    }
    LeaveCriticalSection(&g_notify.lock);

    // If handle is valid cancel any pending notifications and clean up critical secions
    DeleteCriticalSection(&g_notify.lock);
    g_notify.initialized = FALSE;
}

/*
 * Function: WaitForNotification
 * -----------------------------
 * Waits for a notification event from the KMDF driver. If event is 0, waits for any event.
 *
 * Parameters:
 *   UINT32 event - The event code to wait for (0 for any event).
 *
 * Returns:
 *   UINT32 - The event code received, or 0 if none (or error).
 * 
 * Errors:
 *   ERROR_INVALID_HANDLE - Notifications are not initialized thus the handle is invalid.
 */
ECLIB_API
UINT32 WaitForNotification(UINT32 event)
{
    HANDLE hDevice = NULL;
    UINT32 ievent = 0;
    NotificationRsp_t notify_response = {0};
    NotificationReq_t notify_request = {0};
    BOOL aborted = FALSE;

    // Loop until we get event we are looking for
    for(;;) {
        // Make sure Initialization has been done
        // This is checked every iteration because CleanupNotification may have been called which
        // destroys the critical section, and attempting to enter it below is undefined behavior
        if(g_notify.handle == INVALID_HANDLE_VALUE) {
            SetLastError(ERROR_INVALID_HANDLE);
            return 0;
        }  

        // There could be many calls into this function, only first call calls into KMDF driver
        // Subsequent calls just wait for the event to be set by the KMDF driver
        EnterCriticalSection(&g_notify.lock);

        if(!g_notify.in_progress) {
            g_notify.in_progress = TRUE;
            LeaveCriticalSection(&g_notify.lock);

            ULONG bytesReturned;
            notify_request.type = 0x1;
            if(DeviceIoControl ( g_notify.handle,
                                (DWORD) IOCTL_GET_NOTIFICATION,
                                &notify_request,
                                sizeof(notify_request),
                                &notify_response,
                                sizeof( notify_response),
                                &bytesReturned,
                                NULL
                                ) == TRUE )
            {
                g_notify.event = notify_response.lastevent;               
            // Tricky race condition where Cleanup cancels the IO call and we beat it back to the top
            // where we set in_progress to true again, then Cleanup calls cancel again but we haven't
            // entered IoControl call yet, but then we get there and Cleanup is sleeping but now we
            // are stuck in IoControl and can't return to Wake it again resulting in deadlock.
            // So we explicitly check if we returned due to being cancelled and bail out of the loop to prevent this.
            } else if(GetLastError() == ERROR_OPERATION_ABORTED) {
                aborted = TRUE;
            } else {
                g_notify.event = 0;
            }

            // Enter critical section here to ensure wake is caught by Cleanup
            EnterCriticalSection(&g_notify.lock);
            g_notify.in_progress = FALSE;
            WakeAllConditionVariable(&g_notify.cv);
        } else {
            // Wait for notification to be set
            // Loop for spurious wakeups
            while(g_notify.in_progress) {
                SleepConditionVariableCS(&g_notify.cv, &g_notify.lock, INFINITE);
            }
        }

        // Regardless of branch we are always in a critical section at this point so leave it
        LeaveCriticalSection(&g_notify.lock);

        if(aborted || event == 0 || g_notify.event == event) {
            ievent = g_notify.event;
            break;
        }

    } 

    // Return no event
    return ievent;
}