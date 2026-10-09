use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkStatus {
    #[default]
    Unknown,
    Online,
    Offline,
}

pub fn query() -> NetworkStatus {
    #[cfg(windows)]
    {
        match uplink_interfaces() {
            Some(interfaces) if interfaces.is_empty() => NetworkStatus::Offline,
            Some(_) => NetworkStatus::Online,
            None => NetworkStatus::Unknown,
        }
    }
    #[cfg(not(windows))]
    {
        NetworkStatus::Unknown
    }
}

pub fn local_lan_address() -> Option<String> {
    #[cfg(windows)]
    {
        windows_lan_address()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(any(windows, test))]
fn has_uplink(interfaces: impl IntoIterator<Item = (u32, bool)>) -> bool {
    // 排除环回与隧道，否则本应用的 TUN 会把物理断网伪装成仍然在线。
    interfaces
        .into_iter()
        .any(|(kind, up)| up && ![24, 53, 131].contains(&kind))
}

#[cfg(windows)]
fn uplink_interfaces() -> Option<Vec<u32>> {
    use windows_sys::Win32::NetworkManagement::{
        IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2},
        Ndis::IfOperStatusUp,
    };
    let mut table = std::ptr::null_mut::<MIB_IF_TABLE2>();
    if unsafe { GetIfTable2(&mut table) } != 0 || table.is_null() {
        return None;
    }
    struct Table(*mut MIB_IF_TABLE2);
    impl Drop for Table {
        fn drop(&mut self) {
            unsafe {
                FreeMibTable(self.0.cast());
            }
        }
    }
    let table = Table(table);
    let count = unsafe { (*table.0).NumEntries as usize };
    if count > 65536 {
        return None;
    }
    let rows = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*table.0).Table).cast::<MIB_IF_ROW2>(),
            count,
        )
    };
    Some(
        rows.iter()
            .filter(|row| {
                has_uplink([(row.Type, row.OperStatus == IfOperStatusUp)])
                    && row.InterfaceAndOperStatusFlags._bitfield & 1 != 0
            })
            .map(|row| row.InterfaceIndex)
            .collect(),
    )
}

#[cfg(windows)]
fn windows_lan_address() -> Option<String> {
    use windows_sys::Win32::{
        NetworkManagement::IpHelper::{
            FreeMibTable, GetUnicastIpAddressTable, MIB_UNICASTIPADDRESS_ROW,
            MIB_UNICASTIPADDRESS_TABLE,
        },
        Networking::WinSock::{IpDadStatePreferred, AF_INET},
    };
    let interfaces = uplink_interfaces()?;
    let mut table = std::ptr::null_mut::<MIB_UNICASTIPADDRESS_TABLE>();
    if unsafe { GetUnicastIpAddressTable(AF_INET, &mut table) } != 0 || table.is_null() {
        return None;
    }
    struct Table(*mut MIB_UNICASTIPADDRESS_TABLE);
    impl Drop for Table {
        fn drop(&mut self) {
            unsafe {
                FreeMibTable(self.0.cast());
            }
        }
    }
    let table = Table(table);
    let count = unsafe { (*table.0).NumEntries as usize };
    if count > 65536 {
        return None;
    }
    let rows = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*table.0).Table).cast::<MIB_UNICASTIPADDRESS_ROW>(),
            count,
        )
    };
    rows.iter()
        .filter(|row| {
            interfaces.contains(&row.InterfaceIndex)
                && !row.SkipAsSource
                && row.DadState == IpDadStatePreferred
        })
        .filter_map(|row| {
            if unsafe { row.Address.si_family } != AF_INET {
                return None;
            }
            let bytes = unsafe { row.Address.Ipv4.sin_addr.S_un.S_un_b };
            let address = std::net::Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4);
            usable_lan_address(address).then_some(address)
        })
        .min_by_key(|address| (!address.is_private(), u32::from(*address)))
        .map(|address| address.to_string())
}

#[cfg(any(windows, test))]
fn usable_lan_address(address: std::net::Ipv4Addr) -> bool {
    !address.is_loopback()
        && !address.is_link_local()
        && !address.is_unspecified()
        && !address.is_multicast()
        && !address.is_broadcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_detection_ignores_loopback_tun_and_disconnected_interfaces() {
        assert!(!has_uplink([
            (24, true),
            (53, true),
            (131, true),
            (6, false)
        ]));
        assert!(has_uplink([(24, true), (71, true)]));
        assert!(has_uplink([(6, true)]));
        assert!(!has_uplink([]));
    }

    #[test]
    fn lan_candidates_reject_loopback_link_local_and_multicast_addresses() {
        for candidate in [
            "127.0.0.1",
            "0.0.0.0",
            "169.254.1.2",
            "224.1.2.3",
            "255.255.255.255",
        ] {
            assert!(!usable_lan_address(candidate.parse().unwrap()));
        }
        for candidate in ["192.168.1.2", "10.2.3.4", "172.16.0.2", "203.0.113.2"] {
            assert!(usable_lan_address(candidate.parse().unwrap()));
        }
    }
}
