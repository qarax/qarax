#!/bin/bash
# Runs inside virt-customize to pre-install Kubernetes packages into the base disk.
# KUBERNETES_MINOR_PLACEHOLDER is substituted by run.sh before passing to virt-customize.
set -euo pipefail

KUBERNETES_MINOR="KUBERNETES_MINOR_PLACEHOLDER"
PAUSE_VERSION="PAUSE_VERSION_PLACEHOLDER"

cat > /etc/yum.repos.d/kubernetes.repo <<EOF
[kubernetes]
name=Kubernetes
baseurl=https://pkgs.k8s.io/core:/stable:/v${KUBERNETES_MINOR}/rpm/
enabled=1
gpgcheck=1
gpgkey=https://pkgs.k8s.io/core:/stable:/v${KUBERNETES_MINOR}/rpm/repodata/repomd.xml.key
exclude=kubelet kubeadm kubectl cri-tools kubernetes-cni
EOF

dnf install -y containerd kubelet kubeadm kubectl kubernetes-cni \
    --setopt=disable_excludes=kubernetes \
    --setopt=install_weak_deps=False

mkdir -p /etc/containerd
containerd config default > /etc/containerd/config.toml
sed -i 's/SystemdCgroup = false/SystemdCgroup = true/' /etc/containerd/config.toml
sed -i 's/snapshotter = "overlayfs"/snapshotter = "native"/g' /etc/containerd/config.toml
sed -i "s|sandbox_image = \"\"|sandbox_image = \"registry.k8s.io/pause:${PAUSE_VERSION}\"|g" /etc/containerd/config.toml
sed -i "s|sandbox_image = ''|sandbox_image = 'registry.k8s.io/pause:${PAUSE_VERSION}'|g" /etc/containerd/config.toml
# Point containerd to the certs.d directory for per-registry mirror config
# containerd v2 uses single-quoted empty strings in the generated config
sed -i "s|config_path = ''|config_path = '/etc/containerd/certs.d'|g" /etc/containerd/config.toml
sed -i 's|config_path = ""|config_path = "/etc/containerd/certs.d"|g' /etc/containerd/config.toml

systemctl enable containerd kubelet

if systemctl list-unit-files firewalld.service >/dev/null 2>&1; then
    systemctl disable firewalld || true
    systemctl mask firewalld || true
fi

printf 'overlay\nbr_netfilter\n' > /etc/modules-load.d/k8s.conf

cat > /etc/sysctl.d/99-kubernetes.conf <<EOF
net.bridge.bridge-nf-call-iptables  = 1
net.bridge.bridge-nf-call-ip6tables = 1
net.ipv4.ip_forward                  = 1
net.ipv6.bindv6only                  = 0
EOF

sed -i 's/^SELINUX=.*/SELINUX=permissive/' /etc/selinux/config 2>/dev/null || true

sed -i '/\bswap\b/d' /etc/fstab

mkdir -p /etc/systemd
cat > /etc/systemd/zram-generator.conf <<'EOF'
[zram0]
zram-size = 0
EOF
