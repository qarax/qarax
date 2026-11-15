#!/bin/sh
# Creates the certificates the e2e nodes use for live migration TLS
# (qarax-node --migration-tls-dir): a migration CA, a client certificate, and
# for each NODE a server certificate valid for that name, which is the address
# the node is registered under. Writes DIR/NODE/ with the five files Cloud
# Hypervisor reads, unless they are already there.
#
# Usage: gen-migration-tls.sh DIR NODE...
set -eu

dir=$1
shift

missing=
for node in "$@"; do
	[ -f "$dir/$node/client-key.pem" ] || missing=1
done
[ -n "$missing" ] || exit 0

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

# key FILE: a new P-256 private key
key() {
	openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1"
}

# sign OUT CN KEY EXTENSIONS: OUT-cert.pem, a certificate for KEY issued by the CA
sign() {
	printf '%b\n' "$4" >"$1.ext"
	openssl req -new -key "$3" -subj "/CN=$2" -out "$1.csr"
	openssl x509 -req -in "$1.csr" -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial \
		-days 3650 -extfile "$1.ext" -out "$1-cert.pem"
}

key ca-key.pem
openssl req -x509 -new -key ca-key.pem -subj "/CN=qarax e2e migration CA" -days 3650 \
	-addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" \
	-out ca-cert.pem

key client-key.pem
sign client "qarax e2e migration client" client-key.pem \
	'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth'

for node in "$@"; do
	mkdir "$node"
	key "$node/server-key.pem"
	sign "$node/server" "$node" "$node/server-key.pem" \
		"basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:$node"
	rm "$node/server.csr" "$node/server.ext"
	cp ca-cert.pem client-cert.pem client-key.pem "$node/"
	rm -rf "${dir:?}/$node"
	mv "$node" "$dir/"
done
