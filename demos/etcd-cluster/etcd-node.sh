#!/usr/bin/env bash
# etcd-node.sh — started by etcd-node.service
#
# Reads the IP address assigned to eth0 (set by the kernel via the ip= cmdline
# parameter that qarax injects for managed networks), maps it to the etcd node
# name, and starts etcd with the correct peer/client URLs.
#
# Fixed cluster layout — must match the IPs assigned via qarax --ip:
#   etcd-0  10.100.0.10
#   etcd-1  10.100.0.11
#   etcd-2  10.100.0.12

set -euo pipefail

CLUSTER_SUBNET="10.100.0"
CLUSTER_TOKEN="etcd-demo-cluster"
CLUSTER_PEERS="etcd-0=http://10.100.0.10:2380,etcd-1=http://10.100.0.11:2380,etcd-2=http://10.100.0.12:2380"

# Wait up to 30s for eth0 to get an IP
NODE_IP=""
for i in $(seq 1 30); do
    NODE_IP=$(ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[^/]+' || true)
    if [[ -n "$NODE_IP" ]]; then
        break
    fi
    echo "etcd-node: waiting for eth0 IP (attempt $i)..."
    sleep 1
done

if [[ -z "$NODE_IP" ]]; then
    echo "etcd-node: ERROR — could not determine IP address on eth0" >&2
    exit 1
fi

# Map IP to etcd node name
case "$NODE_IP" in
    "${CLUSTER_SUBNET}.10") ETCD_NAME="etcd-0" ;;
    "${CLUSTER_SUBNET}.11") ETCD_NAME="etcd-1" ;;
    "${CLUSTER_SUBNET}.12") ETCD_NAME="etcd-2" ;;
    *)
        echo "etcd-node: ERROR — IP $NODE_IP not in cluster range (${CLUSTER_SUBNET}.10-12)" >&2
        exit 1
        ;;
esac

echo "etcd-node: starting $ETCD_NAME at $NODE_IP"
echo "etcd-node: cluster peers: $CLUSTER_PEERS"

exec /usr/local/bin/etcd \
    --name                        "$ETCD_NAME" \
    --data-dir                    /var/lib/etcd \
    --listen-peer-urls            "http://${NODE_IP}:2380" \
    --listen-client-urls          "http://${NODE_IP}:2379,http://127.0.0.1:2379" \
    --advertise-client-urls       "http://${NODE_IP}:2379" \
    --initial-advertise-peer-urls "http://${NODE_IP}:2380" \
    --initial-cluster             "$CLUSTER_PEERS" \
    --initial-cluster-state       new \
    --initial-cluster-token       "$CLUSTER_TOKEN"
