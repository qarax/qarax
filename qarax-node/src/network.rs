use rand;
use rand::prelude::*;

use std::process::Stdio;
use tokio::process::Command;

const BRIDGE_NAME: &str = "fcbridge";

pub fn generate_mac() -> String {
    let mut buf: [u8; 6] = [0; 6];
    rand::thread_rng().fill_bytes(&mut buf);

    // For locally-administered MAC addresses, the second least significant
    // bit should be 1
    buf[0] |= 2;

    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5]
    )
}

pub async fn get_ip(output: String, mac: &str) -> String {
    let s: String = output.lines().filter(|l| l.contains(mac)).collect();

    s.as_str().split_whitespace().next().unwrap().to_owned()
}

pub async fn create_tap_device(vm_id: &str) {
    // TODO: use a utility or something and handle errors
    let tap_device = &format!("fc-tap-{}", &vm_id[..4]);
    Command::new("ip")
        .args(vec!["tuntap", "add", tap_device, "mode", "tap"])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device")
        .await;

    Command::new("ip")
        .args(vec!["link", "set", tap_device, "up"])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to set tap device up")
        .await;

    Command::new("ip")
        .args(vec!["link", "set", tap_device, "master", BRIDGE_NAME])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device to bridge")
        .await;
}

pub async fn delete_tap_device(vm_id: &str) {
    let tap_device = &format!("fc-tap-{}", &vm_id[..4]);

    Command::new("ip")
        .args(vec!["link", "delete", tap_device])
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to add tap device to bridge")
        .await;
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::runtime::Builder;

    #[test]
    fn test() {
        let mut rt = Builder::new()
            .basic_scheduler()
            .enable_all()
            .build()
            .unwrap();

        let input = String::from("Interface: fcbridge, type: EN10MB, MAC: f6:17:27:50:93:84, IPv4: 192.168.122.45\nStarting arp-scan 1.9.7 with 256 hosts (https://github.com/royhills/arp-scan)\n192.168.122.1\t52:54:00:9b:d5:cc\tQEMU\n\n1 packets received by filter, 0 packets dropped by kernel\nEnding arp-scan 1.9.7: 256 hosts scanned in 1.898 seconds (134.88 hosts/sec). 1 responded\n");
        let out = rt.block_on(get_ip(input, "52:54:00:9b:d5:cc"));
        assert_eq!(out, "192.168.122.1")
    }
}
