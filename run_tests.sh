docker build -t qarax-node-test -f Dockerfile.node .
docker run --rm \
    --privileged \
    --ipc=host \
    --volume /dev:/dev \
    --volume /run/udev/control:/run/udev/control \
    --volume /srv/jailer:/srv/jailer \
    --volume /var/tmp:/var/tmp \
    --device=/dev/vhost-vsock:/dev/vhost-vsock \
    --device=/dev/vsock:/dev/vsock \
    --device=/dev/kvm:/dev/kvm \
    --device=/dev/loop-control:/dev/loop-control \
    qarax-node-tests:latest
