#!/bin/sh
# Creates the certificates for qarax-node's gRPC API, which only serves clients
# with a certificate from its CA: a CA, the control plane's client certificate
# and, for each NODE, a server certificate valid for that address (the one the
# host is registered under, a DNS name or an IP address). Run again with more
# nodes, it issues theirs from the same CA and leaves what exists alone.
#
#   DIR/ca-cert.pem, DIR/ca-key.pem   the CA; keep the key off the hosts
#   DIR/control-plane/                the control plane's node_tls_dir
#   DIR/nodes/NODE/                   qarax-node --tls-dir on NODE
#
# Usage: gen-node-tls.sh DIR NODE...
set -eu

[ $# -ge 1 ] || {
	echo "Usage: $0 DIR NODE..." >&2
	exit 2
}
mkdir -p "$1"
cd "$1"
shift
umask 077

# key FILE: a new P-256 private key
key() {
	openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1" 2>/dev/null
}

# sign CERT KEY CN EXTENSIONS: CERT, a certificate for KEY issued by the CA
sign() {
	printf '%b\n' "$4" >"$1.ext"
	openssl req -new -key "$2" -subj "/CN=$3" -out "$1.csr"
	openssl x509 -req -in "$1.csr" -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial \
		-days 3650 -extfile "$1.ext" -out "$1" 2>/dev/null
	rm "$1.csr" "$1.ext"
}

if [ ! -f ca-key.pem ]; then
	key ca-key.pem
	openssl req -x509 -new -key ca-key.pem -subj "/CN=qarax node CA" -days 3650 \
		-addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" \
		-out ca-cert.pem
fi

if [ ! -f control-plane/client-key.pem ]; then
	mkdir -p control-plane
	key control-plane/client-key.pem
	sign control-plane/client-cert.pem control-plane/client-key.pem "qarax control plane" \
		'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth'
	cp ca-cert.pem control-plane/
fi

for node in "$@"; do
	[ ! -f "nodes/$node/server-key.pem" ] || continue
	case $node in
	*:*) san="IP:$node" ;; # IPv6
	*[!0-9.]*) san="DNS:$node" ;;
	*) san="IP:$node" ;; # IPv4
	esac
	mkdir -p "nodes/$node"
	key "nodes/$node/server-key.pem"
	sign "nodes/$node/server-cert.pem" "nodes/$node/server-key.pem" "$node" \
		"basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=$san"
	cp ca-cert.pem "nodes/$node/"
done
