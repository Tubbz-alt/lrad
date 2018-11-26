#!/bin/bash
die() {
	echo >&2 "$@"
	exit 0
}

[[ -d "${OPENSSL_DIR}/lib" ]] && die "Directory already exists, refusing to build again"
rm -r ${OPENSSL_DIR}
mkdir /tmp/build/
[[ -d /tmp/build ]] || die "Failed to create build directory"
cd /tmp/build/
curl https://www.openssl.org/source/openssl-1.1.1.tar.gz -o /tmp/build/openssl.tar.gz 
[[ -f /tmp/build/openssl.tar.gz ]] || die "Failed to download openssl source code"
tar xzf /tmp/build/openssl.tar.gz && cd openssl-1.1.1
./Configure --prefix=${OPENSSL_DIR} --openssldir=${OPENSSL_DIR} linux-x86_64 -fPIC -g no-shared
make -j$(nproc)
make install
cd ~/build/sameer/lrad
