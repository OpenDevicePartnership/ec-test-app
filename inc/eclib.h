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

#pragma once

#ifdef __cplusplus
#define EXTERN_C extern "C"
#else
#define EXTERN_C
#endif

#define ECLIB_API EXTERN_C __declspec(dllexport)

ECLIB_API int GetKMDFDriverHandle(
    _In_ DWORD flags,
    _Out_ HANDLE *hDevice
);

ECLIB_API int EvaluateAcpi(
    _In_ void* acpi_input,
    _In_ size_t input_len,
    _Out_ BYTE* buffer,
    _In_ size_t* buf_len
);

ECLIB_API
int InitializeNotification();

ECLIB_API
VOID CleanupNotification();

ECLIB_API
UINT32 WaitForNotification(UINT32 event);