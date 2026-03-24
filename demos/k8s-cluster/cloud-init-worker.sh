#!/bin/bash
# Cloud-init setup script for Kubernetes worker nodes.
# Packages (containerd, kubelet, kubeadm) are pre-baked into the disk image
# by prebake.sh via virt-customize. This script only configures networking
# and runs kubeadm join.
# Injected by run.sh: TOKEN_PLACEHOLDER is substituted.
set -euo pipefail
exec >> /var/log/k8s-setup.log 2>&1

TOKEN="TOKEN_PLACEHOLDER"
CONTROL_IP="10.101.0.10"
NODE_IP="NODE_IP_PLACEHOLDER"
GATEWAY="10.101.0.1"
REGISTRY_MIRROR="REGISTRY_IP_PLACEHOLDER:5000"
KUBERNETES_VERSION="KUBERNETES_VERSION_PLACEHOLDER"
PAUSE_VERSION="PAUSE_VERSION_PLACEHOLDER"
IMAGE_ARCHIVE="/var/lib/k8s-image-archives/k8s-images.tar"
CRI_SOCKET="unix:///run/containerd/containerd.sock"
DEBUG_USER="qarax-debug"

echo "=== k8s worker setup start $(date) ==="

log_debug_state() {
    set +e
    echo "=== debug snapshot $(date) ==="
    echo "--- ip addr ---"
    ip addr || true
    echo "--- ip route ---"
    ip route || true
    echo "--- ss -ltnp ---"
    ss -ltnp || true
    echo "--- iptables-save ---"
    iptables-save || true
    echo "--- ip6tables-save ---"
    ip6tables-save || true
    echo "--- nft list ruleset ---"
    nft list ruleset || true
    echo "--- crictl pods ---"
    crictl --runtime-endpoint="${CRI_SOCKET}" pods || true
    echo "--- crictl ps -a ---"
    crictl --runtime-endpoint="${CRI_SOCKET}" ps -a || true
    echo "--- flannel/CNI state ---"
    ls -lah /run/flannel /etc/cni/net.d /opt/cni/bin 2>/dev/null || true
    echo "--- systemctl status ---"
    systemctl --no-pager --full status NetworkManager sshd containerd kubelet 2>&1 | tail -n 200 || true
    echo "--- journalctl ---"
    journalctl -b --no-pager -u NetworkManager -u sshd -u containerd -u kubelet -n 400 || true
}
trap log_debug_state EXIT

enable_debug_access() {
    echo "=== Enabling debug SSH and serial access ==="
    systemctl enable --now sshd || true
    mkdir -p /etc/systemd/system/serial-getty@hvc0.service.d
    cat > /etc/systemd/system/serial-getty@hvc0.service.d/autologin.conf <<EOF
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin ${DEBUG_USER} --keep-baud 115200,38400,9600 %I \$TERM
Type=idle
EOF
    systemctl daemon-reload
    systemctl restart serial-getty@hvc0.service || true
    getent passwd "${DEBUG_USER}" || true
}

disable_swap() {
    echo "=== Disabling swap ==="
    swapoff -a || true
    systemctl stop dev-zram0.swap 2>/dev/null || true
    systemctl mask dev-zram0.swap 2>/dev/null || true
    systemctl stop systemd-zram-setup@zram0.service 2>/dev/null || true
    systemctl mask systemd-zram-setup@zram0.service 2>/dev/null || true
}
disable_swap

setup_network() {
    local iface
    iface=$(ip -o link show | awk -F': ' 'NR>1{split($2,a,"@"); if(a[1]!="lo") {print a[1]; exit}}')
    [[ -z "$iface" ]] && { echo "ERROR: no ethernet interface found"; return 1; }
    echo "Configuring ${NODE_IP}/24 on ${iface} (gw ${GATEWAY}) via nmcli"
    mkdir -p /etc/NetworkManager/system-connections
    cat > /etc/NetworkManager/system-connections/cloud-init-static.nmconnection <<EOF
[connection]
id=cloud-init-static
type=ethernet
interface-name=${iface}
autoconnect=true

[ipv4]
address1=${NODE_IP}/24,${GATEWAY}
dns=8.8.8.8;
method=manual

[ipv6]
method=ignore
EOF
    chmod 600 /etc/NetworkManager/system-connections/cloud-init-static.nmconnection
    nmcli con reload
    nmcli con up cloud-init-static
    echo "Network configured: $(ip addr show dev "${iface}" | grep 'inet ')"
}
setup_network
enable_debug_access

mkdir -p "/etc/containerd/certs.d/${REGISTRY_MIRROR}"
cat > "/etc/containerd/certs.d/${REGISTRY_MIRROR}/hosts.toml" <<TOML
server = "http://${REGISTRY_MIRROR}"

[host."http://${REGISTRY_MIRROR}"]
  capabilities = ["pull", "resolve"]
  skip_verify = true
TOML

if systemctl list-unit-files firewalld.service >/dev/null 2>&1; then
    echo "=== Disabling firewalld ==="
    systemctl disable --now firewalld || true
    systemctl mask firewalld || true
fi
if systemctl list-unit-files nftables.service >/dev/null 2>&1; then
    echo "=== Disabling nftables ==="
    systemctl disable --now nftables || true
    systemctl mask nftables || true
fi
command -v nft >/dev/null 2>&1 && nft flush ruleset || true
command -v iptables >/dev/null 2>&1 && {
    iptables -P INPUT ACCEPT || true
    iptables -P FORWARD ACCEPT || true
    iptables -P OUTPUT ACCEPT || true
    iptables -F || true
    iptables -t nat -F || true
    iptables -t mangle -F || true
    iptables -t raw -F || true
} || true
command -v ip6tables >/dev/null 2>&1 && {
    ip6tables -P INPUT ACCEPT || true
    ip6tables -P FORWARD ACCEPT || true
    ip6tables -P OUTPUT ACCEPT || true
    ip6tables -F || true
    ip6tables -t nat -F || true
    ip6tables -t mangle -F || true
    ip6tables -t raw -F || true
} || true

ensure_cni_plugins() {
    local src
    mkdir -p /opt/cni/bin
    for src in /opt/cni/bin /usr/libexec/cni /usr/lib/cni; do
        [[ -d "$src" ]] || continue
        find "$src" -maxdepth 1 -type f -perm -u+x -exec cp -f {} /opt/cni/bin/ \;
    done
    if [[ ! -x /opt/cni/bin/loopback ]]; then
        echo "ERROR: missing CNI loopback plugin in /opt/cni/bin"
        ls -la /opt/cni/bin || true
        exit 1
    fi
}
ensure_cni_plugins

for i in $(seq 1 60); do
    ping -c 1 -W 2 "${GATEWAY}" &>/dev/null && break
    echo "Waiting for gateway (${i}/60)..."; sleep 3
done
ping -c 1 -W 5 "${GATEWAY}" &>/dev/null || { echo "ERROR: gateway ${GATEWAY} unreachable"; exit 1; }
echo "Gateway reachable"

# Load kernel modules (pre-configured in image, load for current session)
modprobe overlay
modprobe br_netfilter
sysctl -w net.ipv6.bindv6only=0
sysctl --system

MIRROR_AVAILABLE=0
echo "=== Checking registry mirror at http://${REGISTRY_MIRROR} ==="
for i in $(seq 1 15); do
    if curl -sf --connect-timeout 3 --max-time 5 "http://${REGISTRY_MIRROR}/v2/" >/dev/null; then
        MIRROR_AVAILABLE=1
        break
    fi
    echo "Registry mirror not ready yet (${i}/15); retrying..."
    sleep 2
done

if [[ $MIRROR_AVAILABLE -eq 1 ]]; then
    MIRROR_AVAILABLE=1
    echo "=== Patching containerd registry mirror to http://${REGISTRY_MIRROR} ==="
    for reg in registry.k8s.io docker.io quay.io ghcr.io; do
        mkdir -p "/etc/containerd/certs.d/${reg}"
        cat > "/etc/containerd/certs.d/${reg}/hosts.toml" <<TOML
server = "https://${reg}"

[host."http://${REGISTRY_MIRROR}"]
  capabilities = ["pull", "resolve"]
  skip_verify = true
  override_path = true
TOML
    done
    echo "Registry mirror patched"
else
    echo "WARNING: registry mirror ${REGISTRY_MIRROR} is unreachable; using upstream registries"
fi

echo "=== Pre-pulling worker images ==="
for i in $(seq 1 30); do
    [ -S /run/containerd/containerd.sock ] && break
    echo "Waiting for containerd socket (${i}/30)..."; sleep 2
done

OFFLINE_IMAGES_IMPORTED=0
if [[ -f "${IMAGE_ARCHIVE}" ]]; then
    echo "=== Importing offline image archive ${IMAGE_ARCHIVE} ==="
    if ! timeout 300 ctr -n k8s.io images import --no-unpack "${IMAGE_ARCHIVE}"; then
        echo "ERROR: timed out or failed importing ${IMAGE_ARCHIVE}"
        exit 1
    fi
    OFFLINE_IMAGES_IMPORTED=1
    echo "Offline image archive imported"
fi

case "$(uname -m)" in
    x86_64) IMAGE_PLATFORM="linux/amd64" ;;
    aarch64) IMAGE_PLATFORM="linux/arm64" ;;
    *)
        echo "WARNING: unsupported architecture $(uname -m); falling back to containerd default platform selection"
        IMAGE_PLATFORM=""
        ;;
esac

prepull() {
    local local_path="$1" canonical="$2"
    local local_ref="${REGISTRY_MIRROR}/${local_path}"
    local pull_ref="${canonical}"
    local pull_args=()

    if [[ $MIRROR_AVAILABLE -eq 1 ]]; then
        pull_ref="${local_ref}"
        pull_args+=(--plain-http)
    else
        echo "Mirror unavailable; pulling ${canonical} from upstream"
    fi

    if [[ -n "${IMAGE_PLATFORM}" ]]; then
        pull_args+=(--platform "${IMAGE_PLATFORM}")
    fi

    if ctr -n k8s.io images pull "${pull_args[@]}" "${pull_ref}" 2>&1; then
        if [[ "${pull_ref}" != "${canonical}" ]]; then
            ctr -n k8s.io images tag "${pull_ref}" "${canonical}" 2>/dev/null || true
        fi
        echo "Pre-pulled: ${canonical}"
    else
        echo "WARNING: could not pre-pull ${canonical}"
    fi
}
if [[ $OFFLINE_IMAGES_IMPORTED -eq 1 ]]; then
    echo "Offline image archive imported; skipping manual pre-pull"
elif [[ $MIRROR_AVAILABLE -eq 1 ]]; then
    prepull "kube-proxy:${KUBERNETES_VERSION}" "registry.k8s.io/kube-proxy:${KUBERNETES_VERSION}"
    prepull "pause:${PAUSE_VERSION}"            "registry.k8s.io/pause:${PAUSE_VERSION}"
else
    echo "Mirror unavailable; skipping manual pre-pull and deferring to kubeadm join"
fi
echo "=== Pre-pull complete ==="

echo "=== Waiting for control plane at ${CONTROL_IP}:6443 ==="
until curl -sk --connect-timeout 3 --max-time 5 "https://${CONTROL_IP}:6443/healthz" | grep -q ok; do
    sleep 10
done
echo "Control plane is up"

echo "=== Running kubeadm join ==="
kubeadm join "${CONTROL_IP}:6443" \
    --token="${TOKEN}" \
    --discovery-token-unsafe-skip-ca-verification

echo "=== k8s worker setup complete $(date) ==="
echo "k8s-setup-done" > /tmp/k8s-setup-done
