#!/bin/bash

OS_VERSION="fedora-32"
PASSWORD="fedora"
OS_IMG="os.img"
if test -f "$OS_IMG"; then
  echo "$OS_IMG exists, nothing to do"
  exit 0
fi

VIRT_BUILDER=$(which virt-builder)
if [ $? -eq 0 ]; then
  ${VIRT_BUILDER} ${OS_VERSION} --size 8G -o ${OS_IMG} --root-password password:${PASSWORD} \
   --edit '/etc/ssh/sshd_config: s/^#PermitRootLogin prohibit-password/PermitRootLogin yes/'
else
  echo virt-builder not found
  exit 1
fi
