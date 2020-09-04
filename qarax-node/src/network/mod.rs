use rand::prelude::*;
use std::fmt;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use thiserror::Error;
use tokio::process::Command;
use tokio::time::{self, Duration};

pub use std::net::Ipv4Addr as IpAddr;

mod dhcp;

const BRIDGE_NAME: &str = "fcbridge";

#[derive(Error, Debug)]
pub enum MacErrors {
    #[error("Failed to parse string {0}, error: {1}")]
    MacParsingError(String, String),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    pub fn from_bytes(data: [u8; 6]) -> Self {
        let mut bytes = [0; 6];
        bytes.copy_from_slice(&data);
        Self(bytes)
    }
}

impl std::str::FromStr for MacAddress {
    type Err = MacErrors;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let slice: Vec<&str> = s.split(':').collect();
        let mut bytes: [u8; 6] = [0; 6];

        for (i, octect) in slice.iter().enumerate() {
            bytes[i] = u8::from_str_radix(*octect, 16)
                .map_err(|e| MacErrors::MacParsingError(s.to_owned(), e.to_string()))?;
        }

        Ok(Self::from_bytes(bytes))
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let bytes = self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
        )
    }
}

pub fn generate_mac() -> MacAddress {
    let mut buf: [u8; 6] = [0; 6];
    rand::thread_rng().fill_bytes(&mut buf);

    // For locally-administered MAC addresses, the second least significant
    // bit should be 1
    buf[0] |= 2;

    // Set LSB to zero to make it a unicast address
    buf[0] &= 0xFE;

    MacAddress::from_bytes(buf)
}

pub async fn get_ip(mac: Arc<MacAddress>, tap_device: Arc<String>) -> Result<String> {
    let mut timeout = time::delay_for(Duration::from_secs(120));
    loop {
        let tap_device = tap_device.clone();
        let mac = mac.clone();
        tokio::select! {
           _ = &mut timeout => {
               return Err(anyhow!("Could not get IP in time"));
           },
           ip = tokio::spawn(async move {
                dhcp::get_ip(*mac, &tap_device)
            }) => {
                return ip.unwrap();
            }
        };
    }
}

pub async fn create_tap_device(vm_id: &str) -> Result<()> {
    // TODO: use a utility or something
    let tap_device = &format!("fc-tap-{}", &vm_id[..4]);
    Command::new("ip")
        .args(vec!["tuntap", "add", tap_device, "mode", "tap"])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device")
        .await?;

    Command::new("ip")
        .args(vec!["link", "set", tap_device, "up"])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to set tap device up")
        .await?;

    Command::new("ip")
        .args(vec!["link", "set", tap_device, "master", BRIDGE_NAME])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device to bridge")
        .await?;

    Ok(())
}

pub async fn delete_tap_device(vm_id: &str) -> Result<()> {
    let tap_device = &format!("fc-tap-{}", &vm_id[..4]);

    Command::new("ip")
        .args(vec!["link", "delete", tap_device])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device to bridge")
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_mac_address_from_slice() -> Result<()> {
        let mac_string = String::from("b6:dd:3f:6e:a9:1b");
        let mac = MacAddress::from_str(&mac_string)?;

        assert_eq!(mac, MacAddress([0xb6, 0xdd, 0x3f, 0x6e, 0xa9, 0x1b]));
        Ok(())
    }
}
