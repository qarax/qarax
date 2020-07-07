#!/bin/bash

INTERFACE=$(ip route get 8.8.8.8 | awk '{ print $5; exit }')
IP=$(hostname -I)
IP_WITH_CIDR=$(ip addr show dev $INTERFACE | grep "inet " | awk '{print $2}')
ROUTE=$(ip route list dev $INTERFACE | grep -v default | cut -f 1 -d ' ')
DEFAULT_ROUTE=$(ip route list dev $INTERFACE | grep default | awk '{print $3}')

ip link add fcbridge type bridge
ip link set fcbridge up
ip addr flush dev $INTERFACE
ip link set $INTERFACE master fcbridge
ip addr add $IP_WITH_CIDR brd + dev fcbridge
ip route add default via $DEFAULT_ROUTE
ip route add $ROUTE dev fcbridge proto kernel scope link src $IP
exit 0
