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

    #[test]
    fn test_get_platform() {
        let platform = get_platform();
        assert!(!platform.is_empty());
        // Should be one of the known platforms
        assert!(
            platform.contains("linux") || 
            platform.contains("macos") || 
            platform.contains("windows")
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
