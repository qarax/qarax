use rand::prelude::*;
use std::fmt;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::process::Command;
use tokio::time::{self, Duration};

pub use std::net::Ipv4Addr as IpAddr;

mod dhcp;

const BRIDGE_NAME: &str = "fcbridge";

#[derive(Debug, Copy, Clone)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    pub fn from_bytes(data: [u8; 6]) -> Self {
        let mut bytes = [0; 6];
        bytes.copy_from_slice(&data);
        Self(bytes)
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
    buf[0] &= 0b11110000;

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
    // TODO: use a utility or something and handle errors
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
