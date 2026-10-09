use super::{decode_ssid, read_current_ssid, SsidReadError, SsidSnapshot, SsidSource};
use std::{ffi::c_void, mem, ptr};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_INVALID_STATE, ERROR_NOT_FOUND, ERROR_SERVICE_NOT_ACTIVE,
            ERROR_SUCCESS, HANDLE,
        },
        NetworkManagement::WiFi::{
            wlan_interface_state_connected, wlan_intf_opcode_current_connection, WlanCloseHandle,
            WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
            WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST,
        },
    },
};

struct WlanHandle(HANDLE);

impl Drop for WlanHandle {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, ptr::null()) };
    }
}

struct WlanMemory(*mut c_void);

impl Drop for WlanMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WlanFreeMemory(self.0) };
        }
    }
}

struct NativeSource {
    handle: WlanHandle,
    interfaces: Vec<GUID>,
}

impl NativeSource {
    fn open() -> Result<Self, SsidReadError> {
        let mut negotiated_version = 0;
        let mut handle = ptr::null_mut();
        let result =
            unsafe { WlanOpenHandle(2, ptr::null(), &mut negotiated_version, &mut handle) };
        if result != ERROR_SUCCESS {
            return Err(system_error(result));
        }
        if handle.is_null() {
            return Err(SsidReadError::InvalidData);
        }
        Ok(Self {
            handle: WlanHandle(handle),
            interfaces: Vec::new(),
        })
    }
}

impl SsidSource for NativeSource {
    fn interface_states(&mut self) -> Result<Vec<bool>, SsidReadError> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        let result = unsafe { WlanEnumInterfaces(self.handle.0, ptr::null(), &mut list) };
        let allocation = WlanMemory(list.cast());
        if result != ERROR_SUCCESS {
            return Err(system_error(result));
        }
        if allocation.0.is_null() {
            return Err(SsidReadError::InvalidData);
        }
        let count = unsafe { (*list).dwNumberOfItems as usize };
        if count > 4096 {
            return Err(SsidReadError::InvalidData);
        }
        // Win32 返回按 dwNumberOfItems 分配的尾随数组，不能使用声明中占位的单项长度。
        let first = unsafe { ptr::addr_of!((*list).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>() };
        let entries = unsafe { std::slice::from_raw_parts(first, count) };
        self.interfaces = entries.iter().map(|entry| entry.InterfaceGuid).collect();
        Ok(entries
            .iter()
            .map(|entry| entry.isState == wlan_interface_state_connected)
            .collect())
    }

    fn connected_ssid(&mut self, index: usize) -> Result<Option<String>, SsidReadError> {
        let guid = self
            .interfaces
            .get(index)
            .ok_or(SsidReadError::InvalidData)?;
        let mut size = 0;
        let mut data = ptr::null_mut();
        let result = unsafe {
            WlanQueryInterface(
                self.handle.0,
                guid,
                wlan_intf_opcode_current_connection,
                ptr::null(),
                &mut size,
                &mut data,
                ptr::null_mut(),
            )
        };
        let allocation = WlanMemory(data);
        // 枚举后到查询前可能断开或移除适配器，下一次轮询会重新枚举。
        if matches!(result, ERROR_INVALID_STATE | ERROR_NOT_FOUND) {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(system_error(result));
        }
        if allocation.0.is_null() || (size as usize) < mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>()
        {
            return Err(SsidReadError::InvalidData);
        }
        let attributes = unsafe { &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>() };
        if attributes.isState != wlan_interface_state_connected {
            return Ok(None);
        }
        let ssid = &attributes.wlanAssociationAttributes.dot11Ssid;
        decode_ssid(&ssid.ucSSID, ssid.uSSIDLength)
    }
}

fn system_error(code: u32) -> SsidReadError {
    match code {
        ERROR_ACCESS_DENIED => SsidReadError::PermissionDenied,
        // 1722 是 RPC_S_SERVER_UNAVAILABLE，WLAN 服务不可达时可能返回。
        ERROR_SERVICE_NOT_ACTIVE | 1722 => SsidReadError::ServiceUnavailable,
        code => SsidReadError::System(code),
    }
}

pub(super) fn query() -> SsidSnapshot {
    match NativeSource::open() {
        Ok(mut source) => read_current_ssid(&mut source),
        Err(error) => error.snapshot(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_error_codes_preserve_permission_service_and_other_failures() {
        assert_eq!(
            system_error(ERROR_ACCESS_DENIED),
            SsidReadError::PermissionDenied
        );
        assert_eq!(
            system_error(ERROR_SERVICE_NOT_ACTIVE),
            SsidReadError::ServiceUnavailable
        );
        assert_eq!(system_error(1722), SsidReadError::ServiceUnavailable);
        assert_eq!(system_error(87), SsidReadError::System(87));
    }
}
