#!/bin/bash
# Bakes the k8s-cluster demo's base image. run.sh runs it once per cache key
# inside virt-customize (chrooted into the Fedora cloud image) and fills in the
# @NAME@ placeholders.
#
# Installs containerd and the Kubernetes packages, and saves every image the
# cluster runs (kubeadm's list for the installed version plus the images in
# the Flannel manifest) under /var/lib/k8s-images, so the nodes boot without
# pulling anything: node-setup.sh imports the archives.
set -euo pipefail

KUBERNETES_MINOR="@KUBERNETES_MINOR@"
FLANNEL_VERSION="@FLANNEL_VERSION@"
DEMO_DIR=/etc/k8s-demo
IMAGE_DIR=/var/lib/k8s-images

cat >/etc/yum.repos.d/kubernetes.repo <<EOF
[kubernetes]
name=Kubernetes
baseurl=https://pkgs.k8s.io/core:/stable:/v${KUBERNETES_MINOR}/rpm/
enabled=1
gpgcheck=1
gpgkey=https://pkgs.k8s.io/core:/stable:/v${KUBERNETES_MINOR}/rpm/repodata/repomd.xml.key
exclude=kubelet kubeadm kubectl cri-tools kubernetes-cni
EOF

# skopeo copies the images below without a running container runtime.
dnf install -y containerd kubelet kubeadm kubectl kubernetes-cni skopeo \
	--setopt=disable_excludes=kubernetes \
	--setopt=install_weak_deps=False

mkdir -p "$DEMO_DIR" "$IMAGE_DIR"
# The repo serves the newest patch release of the minor; node-setup.sh
# initialises the cluster at exactly this version.
kubeadm version -o short >"${DEMO_DIR}/kubernetes-version"
curl -fsSL --retry 5 -o "${DEMO_DIR}/kube-flannel.yml" \
	"https://github.com/flannel-io/flannel/releases/download/${FLANNEL_VERSION}/kube-flannel.yml"

{
	kubeadm config images list --kubernetes-version "$(<"${DEMO_DIR}/kubernetes-version")"
	sed -nE "s/^[[:space:]]*image:[[:space:]]*['\"]?([^'\"[:space:]]+).*/\1/p" "${DEMO_DIR}/kube-flannel.yml" | sort -u
} >"${IMAGE_DIR}/images.txt"

n=0
while read -r image; do
	n=$((n + 1))
	echo "Saving ${image}"
	skopeo copy --retry-times 3 "docker://${image}" "docker-archive:${IMAGE_DIR}/${n}.tar:${image}" </dev/null
done <"${IMAGE_DIR}/images.txt"

mkdir -p /etc/containerd
containerd config default >/etc/containerd/config.toml
# The sandbox (pause) image is the one kubeadm expects, which is in the archives.
pause_image=$(grep '/pause:' "${IMAGE_DIR}/images.txt")
sed -i -E \
	-e 's/SystemdCgroup = false/SystemdCgroup = true/' \
	-e 's/snapshotter = "overlayfs"/snapshotter = "native"/g' \
	-e "s|^([[:space:]]*sandbox(_image)? = ).*|\1'${pause_image}'|" \
	/etc/containerd/config.toml

systemctl enable containerd kubelet
# Nothing may filter the cluster's traffic (API server, VXLAN, NodePorts).
systemctl mask firewalld.service nftables.service

printf 'overlay\nbr_netfilter\n' >/etc/modules-load.d/k8s.conf

cat >/etc/sysctl.d/99-kubernetes.conf <<EOF
net.bridge.bridge-nf-call-iptables  = 1
net.bridge.bridge-nf-call-ip6tables = 1
net.ipv4.ip_forward                  = 1
net.ipv6.bindv6only                  = 0
EOF

sed -i 's/^SELINUX=.*/SELINUX=permissive/' /etc/selinux/config

# kubelet refuses to run with swap on; Fedora's default is zram swap.
sed -i '/\bswap\b/d' /etc/fstab
cat >/etc/systemd/zram-generator.conf <<'EOF'
[zram0]
zram-size = 0
EOF

# kubernetes-cni installs its plugins into /opt/cni/bin, where Flannel and
# containerd look; Fedora's own plugins (a containerd dependency) live in
# /usr/libexec/cni.
if [[ -d /usr/libexec/cni ]]; then
	find /usr/libexec/cni -maxdepth 1 -type f -perm -u+x -exec cp -f {} /opt/cni/bin/ \;
fi
test -x /opt/cni/bin/loopback

dnf clean all
