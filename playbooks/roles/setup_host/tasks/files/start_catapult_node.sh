#!/bin/bash -e
port=${1:-8001}
./catapult-node serve -p $port &
disown