#!/usr/bin/env bash
# etcd-node.sh: the image's CMD. qarax boots the image from OverlayBD with
# qarax-init as PID 1, which runs this script with the image's ENV.
#
# CLUSTER_MEMBERS lists every member as name=peer-url, comma-separated. run.sh
# bakes it into the image from its node list, so the layout is defined there
# only. This node is the member whose peer URL holds the IP qarax gave eth0
# (through the kernel's ip= parameter, so it is set before init runs).

set -euo pipefail

: "${CLUSTER_MEMBERS:?not set: build the image with demos/etcd-cluster/run.sh}"

addr=$(ip -4 -o addr show dev eth0)
if [[ ! "$addr" =~ inet\ ([0-9.]+)/ ]]; then
	echo "etcd-node: eth0 has no IPv4 address" >&2
	exit 1
fi
node_ip="${BASH_REMATCH[1]}"

name=""
IFS=, read -ra members <<<"$CLUSTER_MEMBERS"
for member in "${members[@]}"; do
	if [[ "$member" == *"//${node_ip}:"* ]]; then
		name="${member%%=*}"
	fi
done
if [[ -z "$name" ]]; then
	echo "etcd-node: ${node_ip} is not a member of ${CLUSTER_MEMBERS}" >&2
	exit 1
fi

echo "etcd-node: starting ${name} at ${node_ip}"

exec /usr/local/bin/etcd \
	--name "$name" \
	--data-dir /var/lib/etcd \
	--listen-peer-urls "http://${node_ip}:2380" \
	--listen-client-urls "http://${node_ip}:2379,http://127.0.0.1:2379" \
	--advertise-client-urls "http://${node_ip}:2379" \
	--initial-advertise-peer-urls "http://${node_ip}:2380" \
	--initial-cluster "$CLUSTER_MEMBERS" \
	--initial-cluster-state new \
	--initial-cluster-token etcd-demo-cluster
