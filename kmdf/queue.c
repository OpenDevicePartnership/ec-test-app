/*++

Copyright (c) 1990-2000  Microsoft Corporation

Module Name:

    queue.c

Abstract:

    This is a C version of a very simple sample driver that illustrates
    how to use the driver framework and demonstrates best practices.

--*/

#include "driver.h"
#include <stdio.h>
#include <acpiioct.h>
#include "wdm.h"
#include "wdmguid.h"
#include "..\inc\ectest.h"

#ifdef ALLOC_PRAGMA
#pragma alloc_text (PAGE, ECTestQueueInitialize)
#endif

#ifdef EC_TEST_NOTIFICATIONS
// Globals
NotificationRsp_t m_NotifyStats = {0};

/**
 * Function: NTSTATUS NotificationCallback
 *
 * Description: 
 * Callback function for handling ACPI notifications.
 *
 * This function is called when an ACPI notification is received. It updates
 * the notification statistics with the current timestamp and the notification
 * value.
 *
 * Parameters:
 * Context - A pointer to the context information for the callback.
 * NotifyValue - The value associated with the ACPI notification.
 *
 * Return Value:
 * VOID
 *
 */
VOID NotificationCallback(
    PVOID Context,
    ULONG NotifyValue
    )
{
    LARGE_INTEGER timestamp;
    WDFREQUEST request = NULL;
    size_t rspSize = 0;
    NotificationRsp_t *rsp = NULL;
    NTSTATUS status = STATUS_SUCCESS;

    KdPrint(("Notification received: %lu\n", NotifyValue));

    KeQuerySystemTimePrecise(&timestamp);

    m_NotifyStats.count++;
    m_NotifyStats.timestamp = timestamp.QuadPart;
    m_NotifyStats.lastevent = NotifyValue;

    WDFDEVICE device = (WDFDEVICE)Context;
    PDEVICE_CONTEXT deviceContext = DeviceContextGet(device);

    WdfWaitLockAcquire(deviceContext->NotificationLock, NULL);
    if (deviceContext->PendingRequest != NULL) {
        request = deviceContext->PendingRequest;
        deviceContext->PendingRequest = NULL;
    }
    WdfWaitLockRelease(deviceContext->NotificationLock);

    if (request != NULL) {
        // Proceed only if the request is not cancelled
        if (STATUS_CANCELLED != WdfRequestUnmarkCancelable(request)) {
            // Retrieve the output buffer from the request
            status = WdfRequestRetrieveOutputBuffer(request, sizeof(NotificationRsp_t), &rsp, &rspSize);
            if (NT_SUCCESS(status)) {
                // Copy the notification data to the output buffer
                RtlCopyMemory(rsp, &m_NotifyStats, sizeof(NotificationRsp_t));

                KdPrint(("Completing 0x%x with Success \n", request));
                WdfRequestCompleteWithInformation(request, STATUS_SUCCESS, sizeof(NotificationRsp_t));
            } else {
                KdPrint(("Completing 0x%x with status %!STATUS!\n", request, status));
                WdfRequestComplete(request, status);
            }
        } else {
            KdPrint(("Request 0x%x was cancelled\n", request));
            // If no request available, just log the notification
            KdPrint(("Not delivered to app : %lu \n", NotifyValue));
        }
    } else {
        // If no request was pending, just log the notification
        KdPrint(("Not delivered to app : %lu \n", NotifyValue));
    }
}

#ifdef ENABLE_NOTIFICATION_SIMULATION
/*
 * Function: VOID TimerCallback
 *
 * Description:
 * Timer routine to simulate receiving the Notification at the driver.
 * This function is called when the timer expires.
 *
 * Parameters:
 * Timer - The Timer object.
 *
 * Return Value:
 * VOID
 *
 */
VOID TimerCallback(WDFTIMER Timer)
{
    WDFDEVICE device = WdfTimerGetParentObject(Timer);

    // Get the current system time
    LARGE_INTEGER systemTime;
    KeQuerySystemTime(&systemTime);

    // Use the low part of the system time as a random value
    ULONG notifyValue = (ULONG)(systemTime.LowPart ^ systemTime.HighPart);
    
    KdPrint(("Notification Triggerd: %lu\n", notifyValue));
    NotificationCallback((PVOID)device, notifyValue);
}
#endif // ENABLE_NOTIFICATION_SIMULATION

/*
 * Function: NTSTATUS SetupNotification
 *
 * Description: 
 * Sets up ACPI notifications for the specified device.
 *
 * Parameters:
 * device - The WDFDEVICE object representing the device.
 *
 * Return Value:
 * NTSTATUS status code indicating the success or failure of the operation.
 *
 */
NTSTATUS SetupNotification(WDFDEVICE device)
{
    ACPI_INTERFACE_STANDARD2 acpiInterface;
    NTSTATUS status = STATUS_SUCCESS;

    status = WdfFdoQueryForInterface(device,
                                     &GUID_ACPI_INTERFACE_STANDARD2,
                                     (PINTERFACE) &acpiInterface,
                                     sizeof(ACPI_INTERFACE_STANDARD2),
                                     1,
                                     NULL);
    
    if (NT_SUCCESS(status)) {
        status = acpiInterface.RegisterForDeviceNotifications(acpiInterface.Context,
                                                              NotificationCallback, 
                                                              NULL);
    }
    return status;
}

/*
 * Function: VOID ECTestEvtRequestCancel
 *
 * Description: 
 * Handles the cancellation of a pending request.
 *
 * Parameters:
 * Request - The WDFREQUEST object representing the request.
 *
 * Return Value:
 * VOID
 *
 */
VOID ECTestEvtRequestCancel(WDFREQUEST Request)
{
    WDFDEVICE device = WdfIoQueueGetDevice(WdfRequestGetIoQueue(Request));
    PDEVICE_CONTEXT deviceContext = DeviceContextGet(device);

    KdPrint(("Cancel Request received for Request 0x%x \n", Request));

    WdfWaitLockAcquire(deviceContext->NotificationLock, NULL);
    if (deviceContext->PendingRequest == Request) {
        KdPrint(("Request found & cleared from pending list\n"));
        deviceContext->PendingRequest = NULL;
    } else {
        KdPrint(("Request not found in pending list\n"));
    }
    WdfWaitLockRelease(deviceContext->NotificationLock);

    KdPrint(("Completing the request 0x%x with STATUS_CANCELLED\n", Request));
    WdfRequestComplete(Request,
                       STATUS_CANCELLED);
}

/*
 * Function: NTSTATUS NotificationGet
 *
 * Description:
 * Handles the NotificationGet request.
 *
 * Parameters:
 * DeviceObject - The WDFDEVICE object representing the device.
 * Request - The WDFREQUEST object representing the request.
 *
 * Return Value:
 * NTSTATUS status code indicating the success or failure of the operation.
 *
 */
NTSTATUS NotificationGet(WDFDEVICE Device, WDFREQUEST Request)
{
    PDEVICE_CONTEXT deviceContext = DeviceContextGet(Device);

    WdfWaitLockAcquire(deviceContext->NotificationLock, NULL);
    if (deviceContext->PendingRequest != NULL) {
        WdfWaitLockRelease(deviceContext->NotificationLock);

        KdPrint(("Request 0x%x already pending\n", deviceContext->PendingRequest));
        // If a request is already pending, complete the new request with STATUS_DEVICE_BUSY
        return STATUS_DEVICE_BUSY;
    }

    // Keeping this simple. Only one request can be pended at a time (since only 1 app is supported at a time)
    deviceContext->PendingRequest = Request;
    KdPrint(("Saving Request 0x%x to pending list\n", deviceContext->PendingRequest));
    WdfWaitLockRelease(deviceContext->NotificationLock);

    WdfRequestMarkCancelable(Request, ECTestEvtRequestCancel);
    return STATUS_PENDING;
}
#endif // EC_TEST_NOTIFICATIONS
/*
 * Function: NTSTATUS ECTestQueueInitialize
 *
 * Description:
 * The ECTestQueueInitialize function configures and creates a default I/O queue for a specified device.
 * It sets up the queue to handle device control requests and ensures that requests not forwarded to other queues are dispatched here.
 *
 * Parameters:
 * WDFDEVICE Device: A handle to the framework device object.
 *
 * Return Value:
 * Returns an NTSTATUS value indicating the success or failure of the queue creation.
 * If the queue is successfully created, it returns STATUS_SUCCESS. Otherwise, it returns an appropriate error code.
 */
NTSTATUS
ECTestQueueInitialize(
    WDFDEVICE Device
    )
{
    NTSTATUS status;
    WDF_IO_QUEUE_CONFIG    queueConfig;

    PAGED_CODE();

    //
    // Configure a default queue so that requests that are not
    // configure-fowarded using WdfDeviceConfigureRequestDispatching to goto
    // other queues get dispatched here.
    // NOTE: Dispatch is parallel to allow app to pend a notification requests along 
    // with DSM request
    //
    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(
        &queueConfig,
#ifdef EC_TEST_NOTIFICATIONS
        WdfIoQueueDispatchParallel
#else
        WdfIoQueueDispatchSequential
#endif
        );

    queueConfig.EvtIoDeviceControl = ECTestEvtIoDeviceControl;

    status = WdfIoQueueCreate(
                 Device,
                 &queueConfig,
                 WDF_NO_OBJECT_ATTRIBUTES,
                 WDF_NO_HANDLE
                 );

    if( !NT_SUCCESS(status) ) {
        KdPrint(("WdfIoQueueCreate failed 0x%x\n",status));
        return status;
    }

#ifdef EC_TEST_NOTIFICATIONS
    status = SetupNotification(Device);
#endif // EC_TEST_NOTIFICATIONS

    return status;
}

/*
 * Function: VOID WorkItemCallback
 *
 * Description:
 * The WorkItemCallback function is a callback function that processes a work item in a KMDF driver.
 * It retrieves the output buffer, creates a preallocated memory object, and sends an internal IOCTL request to the device.
 * The function then completes the request with the appropriate status and information.
 *
 * Parameters:
 * WDFWORKITEM WorkItem: A handle to the work item being processed.
 *
 * Return Value:
 * This function does not return a value.
 */
VOID
WorkItemCallback(
    _In_ WDFWORKITEM WorkItem
    )
{
    PWORKITEM_CONTEXT context = WorkItemGetContext(WorkItem);

    ACPI_EVAL_INPUT_BUFFER_V1_EX *inputBuffer = NULL;
    WDF_MEMORY_DESCRIPTOR inputMemDesc;
    WDF_MEMORY_DESCRIPTOR outputMemDesc;
    WDFMEMORY outputMemory = WDF_NO_HANDLE;
    WDF_OBJECT_ATTRIBUTES attributes;
    ULONG BytesReturned = 0;
    NTSTATUS status = STATUS_SUCCESS;
    PCHAR outBuf = NULL;
    size_t outSize = 0;
    size_t bufSize = 0;

    status = WdfRequestRetrieveInputBuffer(context->Request, 0, &inputBuffer, &bufSize);
    if(!NT_SUCCESS(status)) {
        status = STATUS_INSUFFICIENT_RESOURCES;
        goto Cleanup;
    }

    // Determine the size of output buffer and only give this much space to ACPI request
    status = WdfRequestRetrieveOutputBuffer(context->Request, 0, &outBuf, &outSize);
    if(!NT_SUCCESS(status)) {
        status = STATUS_INSUFFICIENT_RESOURCES;
        goto Cleanup;
    }

    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.ParentObject = context->Device;
    status = WdfMemoryCreatePreallocated(&attributes,
                                        outBuf,
                                        outSize,
                                        &outputMemory);

    if(!NT_SUCCESS(status)) {
        status = STATUS_INSUFFICIENT_RESOURCES;
        goto Cleanup;
    }

    WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&inputMemDesc, inputBuffer, sizeof(ACPI_EVAL_INPUT_BUFFER_V1_EX));
    WDF_MEMORY_DESCRIPTOR_INIT_HANDLE(&outputMemDesc, outputMemory, NULL);
    
    status = WdfIoTargetSendInternalIoctlSynchronously(
                 WdfDeviceGetIoTarget(context->Device),
                 NULL,
                 IOCTL_ACPI_EVAL_METHOD_EX,
                 &inputMemDesc,
                 &outputMemDesc,
                 NULL,
                 (PULONG_PTR)&BytesReturned);

#if defined(EC_TEST_NOTIFICATIONS) && defined(ENABLE_NOTIFICATION_SIMULATION)
    if (NT_SUCCESS(status)) {
        PDEVICE_CONTEXT deviceContext = DeviceContextGet(context->Device);
        if(deviceContext->Timer != NULL) {
            // Toggle the timer
            if (FALSE == WdfTimerStart(deviceContext->Timer, WDF_REL_TIMEOUT_IN_MS(200))) {
                KdPrint(("Starting Notification Simulation timer\n"));
            } else{
                KdPrint(("Stopping Notification Simulation timer\n"));
                WdfTimerStop(deviceContext->Timer, FALSE);
            }
        }
    }
#endif

    if(!NT_SUCCESS(status)) {
        status = STATUS_INVALID_PARAMETER;
    }

Cleanup:
    WdfRequestSetInformation(context->Request,BytesReturned);
    WdfRequestComplete( context->Request, status);
}

/*
 * Function: NTSTATUS CreateAndEnqueueWorkItem
 *
 * Description:
 * The CreateAndEnqueueWorkItem function creates a work item and enqueues it for execution.
 * It initializes the work item configuration and context, and sets the callback function for the work item.
 *
 * Parameters:
 * WDFDEVICE Device: A handle to the framework device object.
 * WDFREQUEST Request: A handle to the framework request object.
 *
 * Return Value:
 * Returns an NTSTATUS value indicating the success or failure of the work item creation and enqueueing.
 * If the work item is successfully created and enqueued, it returns STATUS_SUCCESS. Otherwise, it returns an appropriate error code.
 */
NTSTATUS
CreateAndEnqueueWorkItem(
    _In_ WDFDEVICE Device,
    _In_ WDFREQUEST Request
    )
{
    NTSTATUS status;
    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_WORKITEM_CONFIG workitemConfig;
    WDFWORKITEM workItem;
    PWORKITEM_CONTEXT context;

    WDF_WORKITEM_CONFIG_INIT(&workitemConfig, WorkItemCallback);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, WORKITEM_CONTEXT);
    attributes.ParentObject = Device;

    status = WdfWorkItemCreate(&workitemConfig, &attributes, &workItem);
    if (!NT_SUCCESS(status)) {
        KdPrint(("WdfWorkItemCreate failed: %!STATUS!\n", status));
        return status;
    }

    context = WorkItemGetContext(workItem);
    context->Device = Device;
    context->Request = Request;

    WdfWorkItemEnqueue(workItem);

    return status;
}

/*
 * Function: VOID ECTestEvtIoDeviceControl
 *
 * Description:
 * The ECTestEvtIoDeviceControl function handles device control requests for a KMDF driver.
 * It processes the specified I/O control code and enqueues a work item for ACPI method evaluation if applicable.
 *
 * Parameters:
 * WDFQUEUE Queue: A handle to the framework queue object.
 * WDFREQUEST Request: A handle to the framework request object.
 * size_t OutputBufferLength: The length of the output buffer.
 * size_t InputBufferLength: The length of the input buffer.
 * ULONG IoControlCode: The I/O control code specifying the operation to perform.
 *
 * Return Value:
 * This function does not return a value.
 */
VOID
ECTestEvtIoDeviceControl(
    IN WDFQUEUE         Queue,
    IN WDFREQUEST       Request,
    IN size_t           OutputBufferLength,
    IN size_t           InputBufferLength,
    IN ULONG            IoControlCode
    )
{
    NTSTATUS            status = STATUS_SUCCESS;// Assume success
    BOOLEAN             completeRequest = TRUE;

    if(!OutputBufferLength || !InputBufferLength)
    {
        WdfRequestComplete(Request, STATUS_INVALID_PARAMETER);
        return;
    }

    WDFDEVICE device = WdfIoQueueGetDevice(Queue);

    //
    // Determine which I/O control code was specified.
    //

    switch (IoControlCode)
    {
    case IOCTL_ACPI_EVAL_METHOD_EX:
        KdPrint(("IOCTL_ACPI_EVAL_METHOD_EX\n"));
        // For bufffered ioctls WdfRequestRetrieveInputBuffer &
        // WdfRequestRetrieveOutputBuffer return the same buffer
        // pointer (Irp->AssociatedIrp.SystemBuffer), so read the
        // content of the buffer before writing to it.
        size_t bufSize = 0;
        ACPI_EVAL_INPUT_BUFFER_V1_EX *InputBuffer = NULL;

        status = WdfRequestRetrieveInputBuffer(Request, 0, &InputBuffer, &bufSize);
        if(!NT_SUCCESS(status)) {
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }
    
        // Check to make sure this buffer was populated properly
        if(InputBuffer->Signature != ACPI_EVAL_INPUT_BUFFER_SIGNATURE_EX)
        {
            status = STATUS_INVALID_PARAMETER;
            break;
        }

        // Don't complete the request here it will be completed in the callback

        status = CreateAndEnqueueWorkItem(device, Request);
        // If we enqueue it successfully it will be completed later, otherwise complete with status
        if (NT_SUCCESS(status)) {
            KdPrint(("EVAL request 0x%x pended\n", Request));
            // Request will be completed later in work item callback

            completeRequest = FALSE;
        } else {
            KdPrint(("CreateAndEnqueueWorkItem failed\n"));
        }
        break;
#ifdef EC_TEST_NOTIFICATIONS
    case IOCTL_GET_NOTIFICATION:
        KdPrint(("IOCTL_GET_NOTIFICATION \n"));
        status = NotificationGet(device, Request);

        // If we enqueue it successfully it will be completed later, otherwise complete with status
        if (NT_SUCCESS(status)) {
            completeRequest = FALSE;
        }
        break;
#endif // EC_TEST_NOTIFICATIONS

#ifdef EC_TEST_SHARED_BUFFER
    case IOCTL_READ_RX_BUFFER:
        size_t rxSize = 0;
        RxBufferRsp_t *rxrsp = NULL;

        // Determine the size of output buffer and only give this much space to ACPI request
        status = WdfRequestRetrieveOutputBuffer(Request, 0, &rxrsp, &rxSize);
        if(!NT_SUCCESS(status)) {
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }

        PHYSICAL_ADDRESS physicalAddress;
        PVOID virtualAddress;
        ULONG64 value;

        // Set the physical address
        physicalAddress.QuadPart = SBSAQEMU_SHARED_MEM_BASE;

        // Map the physical address to a virtual address
        virtualAddress = MmMapIoSpaceEx(physicalAddress, sizeof(ULONG64), PAGE_READONLY);

        if (virtualAddress == NULL) {
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }

        // Read the value from the virtual address
        value = *(volatile ULONG64*)virtualAddress;

        // Unmap the virtual address
        MmUnmapIoSpace(virtualAddress, sizeof(ULONG64));
        
        rxrsp->data = value;
        break;
#endif // EC_TEST_SHARED_BUFFER

    default:
        status = STATUS_INVALID_PARAMETER;
        break;
    }

    if (completeRequest) {
        WdfRequestComplete(Request, status);
    }
}
