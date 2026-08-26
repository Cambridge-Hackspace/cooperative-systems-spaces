use anyhow::{Context, Result};
use serde::Serialize;
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub mac_address: String,
    pub platform: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub software_version: String,
}

impl SystemInfo {
    /// Collect current system information
    pub fn collect() -> Self {
        let mac_address = get_mac_address().unwrap_or_else(|_| "unknown".to_string());
        let platform = get_platform();
        let (ipv4_address, ipv6_address) = get_ip_addresses();

        SystemInfo {
            mac_address,
            platform,
            ipv4_address,
            ipv6_address,
            software_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Get system information for device registration
pub fn get_system_info() -> Result<SystemInfo> {
    let mac_address = get_mac_address()?;
    let platform = get_platform();
    let (ipv4_address, ipv6_address) = get_ip_addresses();

    Ok(SystemInfo {
        mac_address,
        platform,
        ipv4_address,
        ipv6_address,
        software_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get the first non-loopback MAC address
fn get_mac_address() -> Result<String> {
    let mac = mac_address::get_mac_address()
        .context("Failed to retrieve MAC address")?
        .ok_or_else(|| anyhow::anyhow!("No MAC address found"))?;

    Ok(format!("{}", mac))
}

/// Get platform information
fn get_platform() -> String {
    #[cfg(all(target_os = "linux"))]
    return "Linux".to_string();

    #[cfg(all(target_os = "macos"))]
    return "MacOs".to_string();

    #[cfg(all(target_os = "windows"))]
    return "Windows".to_string();

    #[cfg(not(any(
        all(target_os = "linux"),
        all(target_os = "macos"),
        all(target_os = "windows")
    )))]
    return "Other".to_string();
}

/// Get IPv4 and IPv6 addresses
fn get_ip_addresses() -> (Option<String>, Option<String>) {
    let mut ipv4 = None;
    let mut ipv6 = None;

    if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
        for iface in interfaces {
            // Skip loopback interfaces
            if iface.is_loopback() {
                continue;
            }

            match iface.addr.ip() {
                IpAddr::V4(addr) => {
                    if ipv4.is_none() {
                        ipv4 = Some(addr.to_string());
                    }
                }
                IpAddr::V6(addr) => {
                    if ipv6.is_none() && !addr.is_loopback() {
                        ipv6 = Some(addr.to_string());
                    }
                }
            }

            // Break if we found both
            if ipv4.is_some() && ipv6.is_some() {
                break;
            }
        }
    }

    (ipv4, ipv6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_mac_address() {
        let result = get_mac_address();
        // This should work on most systems
        assert!(result.is_ok());
        if let Ok(mac) = result {
            // MAC address should contain colons
            assert!(mac.contains(':'));
        }
    }

    /// Every value `get_platform` can return, and the `space_device_platform`
    /// value the server will store it as.
    ///
    /// Deliberately written out here rather than imported: this is the *edge*
    /// side's independent statement of a contract the *server* also states, and
    /// a check derived from the thing it checks agrees with itself no matter
    /// what. `checks/` asserts this list against the SQL enum in
    /// `server/migrations/2025-12-04-015018-0000_add_devices/up.sql`.
    const PLATFORM_CONTRACT: &[(&str, &str)] = &[
        ("Linux", "linux"),
        ("MacOs", "macos"),
        ("Windows", "windows"),
        ("Other", "other"),
    ];

    /// The contract `get_platform` actually has, which is not "one of the three
    /// desktop operating systems".
    ///
    /// `POST /api/devices/register` lowercases this string and matches it
    /// against `space_device_platform` — `('windows','linux','macos','other')` —
    /// returning 400 for anything else (`server/src/api/devices.rs:192-198`).
    /// So the real requirement is that the value always survives that match.
    ///
    /// The previous assertion required `Linux`/`MacOs`/`Windows` and therefore
    /// failed on every host the `Other` arm exists to serve — FreeBSD, which is
    /// what this project's own development workstation runs. Widening it to
    /// admit `Other` would have been the weakening; asserting the round trip is
    /// strictly stronger, because it also rejects a `get_platform` that returned
    /// `"Other"` on Linux, which the old test happily allowed.
    #[test]
    fn get_platform_always_survives_the_server_side_match() {
        let platform = get_platform();

        let (_, stored) = PLATFORM_CONTRACT
            .iter()
            .find(|(returned, _)| *returned == platform)
            .unwrap_or_else(|| {
                panic!(
                    "get_platform() returned {platform:?}, which POST /api/devices/register \
                     would reject with 400. Adding a platform means adding it to \
                     space_device_platform in a migration first."
                )
            });

        assert_eq!(
            platform.to_lowercase(),
            *stored,
            "the server lowercases this string before matching the SQL enum"
        );
    }

    /// And that it reports the *right* one, not merely a valid one.
    ///
    /// Only asserted for the three platforms the function names explicitly;
    /// every other target is `Other` by construction, which is what this host
    /// is and what the assertion below therefore checks here.
    #[test]
    fn get_platform_agrees_with_the_compilation_target() {
        let expected = match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "MacOs",
            "windows" => "Windows",
            _ => "Other",
        };
        assert_eq!(
            get_platform(),
            expected,
            "built for {}",
            std::env::consts::OS
        );
    }

    #[test]
    fn test_get_system_info() {
        let result = get_system_info();
        assert!(result.is_ok());

        if let Ok(info) = result {
            assert!(!info.mac_address.is_empty());
            assert!(!info.platform.is_empty());
            // IP addresses may or may not be available depending on the system
        }
    }
}
