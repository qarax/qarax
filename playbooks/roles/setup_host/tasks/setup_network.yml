- name: set ipv4 forwarding
  sysctl:
    name: net.ipv4.conf.all.forwarding
    value: 1
    state: present

- name: proxy arp
  sysctl:
    name: net.ipv4.conf.all.proxy_arp
    value: 1
    state: present

- name: disable ipv6
  sysctl:
    name: net.ipv6.conf.all.disable_ipv6
    value: 1
    state: present

- name: copy setup_bridge.sh to host
  copy:
    src: ./files/setup_bridge.sh
    dest: setup_bridge.sh
    mode: u=rwx

- name: check if bridge present
  shell: ip addr show dev fcbridge | grep fcbridge
  register: fcbridge
  ignore_errors: true

- name: run bridge setup
  shell: ./setup_bridge.sh
  when: fcbridge.rc == 1
