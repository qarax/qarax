#!/bin/bash
echo "Defining VM..."

VM_NAME=${1:-"fchost"}
LIBVIRT_NETWORK=${2:-"default"}
VM_IP=${3:-"192.168.122.45"}

if virsh list --all | grep -q "${VM_NAME}"; then
    echo "${VM_NAME} is already installed... "
else
    dom=$(virt-install --import --name "${VM_NAME}" \
        --memory 1024 --vcpus 1 --cpu host \
        --disk os.img,bus=virtio \
        --os-type=linux \
        --graphics spice \
        --noautoconsole \
        --network=default,model=virtio \
        --connect qemu:///system \
        --print-xml)
    echo $dom | virsh define /dev/stdin
fi

fc_host_status=$(virsh list | grep fc_host | tr -s \"[:blank:]\" | cut -d ' ' -f4)
if [  "${fc_host_status}" == 'running' ]; then
    echo "${VM_NAME} is already running"
    exit 0
fi

mac_address=$(virsh dumpxml "${VM_NAME}" | grep "mac address" | awk -F\' '{ print $2}')
echo "Setting IP address to ${VM_IP} for MAC address ${mac_address}"

xml_entry="<host mac=\"${mac_address}\" name=\"${VM_NAME}\" ip=\"${VM_IP}\"/>"
existing_entry=$(virsh net-dumpxml "${LIBVIRT_NETWORK}" | grep "${VM_NAME}")
quote_stripped_entry=$(echo $existing_entry | sed -e 's/\"//g')

if [[ "$xml_entry" == "$quote_stripped_entry" ]]; then
    echo "IP address is already configured"
else
    if [[ -n "$existing_entry" ]]; then
        existing_mac=$(virsh net-dumpxml --network default | grep fchost | awk -F\' '{ print $2}')
        if [[ "$existing_mac" != "$mac_address" ]]; then
            echo "Removing existing entry..."
            existing_entry=$(echo "${existing_entry}" | sed -e 's/^[[:space:]]*//')
            virsh net-update ${LIBVIRT_NETWORK} delete ip-dhcp-host "${existing_entry}" --live --config

            echo "Removing existing lease..."
            dhcp_release virbr0 "${VM_IP}" "${existing_mac}"
        fi
    fi

    echo "Adding DHCP entry to ${LIBVIRT_NETWORK} network..."
    virsh net-update ${LIBVIRT_NETWORK} add-last ip-dhcp-host "${xml_entry}" --live --config
fi

echo "starting ${VM_NAME}..."
virsh start "${VM_NAME}"
