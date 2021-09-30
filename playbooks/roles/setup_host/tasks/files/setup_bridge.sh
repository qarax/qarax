#!/bin/bash

BRIDGE=fcbridge
INTERFACE=$(ip route get 8.8.8.8 | awk '{ print $5; exit }')
IP=$(hostname -I)
IP_WITH_CIDR=$(ip addr show dev $INTERFACE | grep "inet " | awk '{print $2}')
ROUTE=$(ip route list dev $INTERFACE | grep -v default | cut -f 1 -d ' ')
DEFAULT_ROUTE=$(ip route list dev $INTERFACE | grep default | awk '{print $3}')

nmcli con add ifname $BRIDGE type bridge con-name $BRIDGE
nmcli con add type bridge-slave ifname $INTERFACE master $BRIDGE
nmcli con down "System $INTERFACE"
nmcli con mod $BRIDGE ipv4.addresses $IP_WITH_CIDR
nmcli con up $BRIDGE

exit 0
