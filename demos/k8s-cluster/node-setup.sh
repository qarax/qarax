#!/bin/bash
# First-boot setup for one node of the k8s-cluster demo. cloud-init runs it
# once (runcmd); run.sh fills in the @NAME@ placeholders.
#
# prebake.sh already put the packages, the Flannel manifest and every image
# the cluster runs into the disk, so this script only configures the node and
# runs kubeadm init (control) or kubeadm join (worker).
#
# Output goes to /var/log/k8s-setup.log. /var/lib/k8s-setup/status reads
# "running", then "done" or "failed"; run.sh polls it over SSH.
set -Eeuo pipefail

ROLE="@ROLE@" # control or worker
NODE_IP="@NODE_IP@"
CONTROL_IP="@CONTROL_IP@"
GATEWAY="@GATEWAY@"
TOKEN="@TOKEN@"
DEBUG_USER="@DEBUG_USER@"
POD_CIDR="10.244.0.0/16"
CRI_SOCKET="unix:///run/containerd/containerd.sock"
STATUS_FILE=/var/lib/k8s-setup/status

mkdir -p "${STATUS_FILE%/*}"
echo running >"$STATUS_FILE"
exec >>/var/log/k8s-setup.log 2>&1
echo "=== k8s ${ROLE} setup start $(date) ==="

# State for a post-mortem, in its own file so the tail of k8s-setup.log
# still ends with the command that failed.
debug_snapshot() {
	set +e
	trap - ERR
	{
		echo "=== debug snapshot $(date) ==="
		echo "--- ip addr ---"
		ip addr
		echo "--- ip route ---"
		ip route
		echo "--- ss -ltnp ---"
		ss -ltnp
		echo "--- iptables-save ---"
		iptables-save
		echo "--- nft list ruleset ---"
		nft list ruleset
		echo "--- crictl pods ---"
		crictl --runtime-endpoint="${CRI_SOCKET}" pods
		echo "--- crictl ps -a ---"
		crictl --runtime-endpoint="${CRI_SOCKET}" ps -a
		echo "--- CNI state ---"
		ls -lah /run/flannel /etc/cni/net.d /opt/cni/bin
		echo "--- systemctl status ---"
		systemctl --no-pager --full status NetworkManager sshd containerd kubelet | tail -n 200
		echo "--- journalctl ---"
		journalctl -b --no-pager -u NetworkManager -u sshd -u containerd -u kubelet -n 400
		if [[ -f /etc/kubernetes/admin.conf ]]; then
			echo "--- kubectl get pods -A -o wide ---"
			kubectl --kubeconfig=/etc/kubernetes/admin.conf get pods -A -o wide
			echo "--- kubectl get nodes -o wide ---"
			kubectl --kubeconfig=/etc/kubernetes/admin.conf get nodes -o wide
		fi
	} >>/var/log/k8s-setup-debug.log 2>&1
}

finish() {
	local rc=$?
	if ((rc == 0)); then
		echo "=== k8s ${ROLE} setup complete $(date) ==="
		echo "done" >"$STATUS_FILE"
	else
		echo "=== k8s ${ROLE} setup FAILED (exit ${rc}) $(date); debug snapshot in /var/log/k8s-setup-debug.log ==="
		debug_snapshot
		echo failed >"$STATUS_FILE"
	fi
}
trap 'echo "FAILED at line ${LINENO}: ${BASH_COMMAND}"' ERR
trap finish EXIT

# retry TRIES DELAY CMD...: run CMD until it succeeds, at most TRIES times.
retry() {
	local tries="$1" delay="$2" i
	shift 2
	for ((i = 1; i <= tries; i++)); do
		"$@" && return 0
		sleep "$delay"
	done
	echo "Gave up after ${tries} tries: $*"
	return 1
}

# Static IP through NetworkManager rather than cloud-init network-config,
# which is unreliable on Fedora 43 with NetworkManager.
configure_network() {
	local iface
	iface=$(ip -o link show | awk -F': ' 'NR>1{split($2,a,"@"); if(a[1]!="lo") {print a[1]; exit}}')
	[[ -n "$iface" ]] || {
		echo "No ethernet interface found"
		return 1
	}
	echo "Configuring ${NODE_IP}/24 on ${iface} (gateway ${GATEWAY})"
	cat >/etc/NetworkManager/system-connections/cloud-init-static.nmconnection <<EOF
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
	retry 60 2 ping -c 1 -W 2 "$GATEWAY"
}

# `qarax vm attach` connects to the serial console: log the debug user in there.
enable_debug_access() {
	systemctl enable --now sshd
	mkdir -p /etc/systemd/system/serial-getty@ttyS0.service.d
	cat >/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf <<EOF
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin ${DEBUG_USER} --keep-baud 115200,38400,9600 %I \$TERM
Type=idle
EOF
	systemctl daemon-reload
	# A debugging convenience only: don't fail the node over it.
	systemctl restart serial-getty@ttyS0.service || echo "Serial console autologin unavailable"
}

prepare_node() {
	swapoff -a
	modprobe overlay
	modprobe br_netfilter
	sysctl --system
	retry 30 1 test -S /run/containerd/containerd.sock
	local archive
	for archive in /var/lib/k8s-images/*.tar; do
		echo "Importing ${archive}"
		timeout 300 ctr -n k8s.io images import --no-unpack "$archive"
	done
}

init_control_plane() {
	local version
	version=$(</etc/k8s-demo/kubernetes-version)
	cat >/etc/k8s-demo/kubeadm-init.yaml <<EOF
apiVersion: kubeadm.k8s.io/v1beta4
kind: InitConfiguration
localAPIEndpoint:
  advertiseAddress: ${NODE_IP}
  bindPort: 6443
nodeRegistration:
  criSocket: ${CRI_SOCKET}
  kubeletExtraArgs:
    - name: node-ip
      value: "${NODE_IP}"
bootstrapTokens:
  - token: ${TOKEN}
---
apiVersion: kubeadm.k8s.io/v1beta4
kind: ClusterConfiguration
kubernetesVersion: ${version}
networking:
  podSubnet: ${POD_CIDR}
apiServer:
  extraArgs:
    - name: advertise-address
      value: "${NODE_IP}"
    - name: bind-address
      value: "0.0.0.0"
EOF
	cat /etc/k8s-demo/kubeadm-init.yaml
	kubeadm init --config=/etc/k8s-demo/kubeadm-init.yaml --skip-phases=preflight -v=5
	mkdir -p /root/.kube
	cp /etc/kubernetes/admin.conf /root/.kube/config
	kubectl --kubeconfig=/etc/kubernetes/admin.conf apply --validate=false -f /etc/k8s-demo/kube-flannel.yml
}

join_cluster() {
	echo "Waiting for the API server at ${CONTROL_IP}:6443"
	retry 180 5 curl -fsk --max-time 5 "https://${CONTROL_IP}:6443/healthz"
	kubeadm join "${CONTROL_IP}:6443" \
		--token="$TOKEN" \
		--discovery-token-unsafe-skip-ca-verification
}

configure_network
enable_debug_access
prepare_node
case "$ROLE" in
control) init_control_plane ;;
worker) join_cluster ;;
*)
	echo "Unknown role: ${ROLE}"
	exit 1
	;;
esac
