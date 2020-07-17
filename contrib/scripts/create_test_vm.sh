#!/bin/bash

OS_VERSION="fedora-32"
PASSWORD="fedora"
OS_IMG="os.img"
if test -f "$FILE"; then
  echo "$FILE exists, nothing to do"
  exit 0
fi

VIRT_BUILDER=$(which virt-builder)
if [ $? -eq 0 ]; then
  ${VIRT_BUILDER} ${OS_VERSION} --size 8G -o ${OS_IMG} --root-password password:${PASSWORD}
else
  echo virt-builder not found
  exit 1
fi
