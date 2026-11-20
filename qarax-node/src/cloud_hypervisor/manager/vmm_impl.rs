use super::*;

#[async_trait::async_trait]
impl crate::vmm::VmmManager for VmManager {
    async fn create_vm(
        &self,
        config: ProtoVmConfig,
    ) -> Result<crate::rpc::node::VmState, crate::vmm::VmmError> {
        VmManager::create_vm(self, config).await.map_err(Into::into)
    }

    async fn start_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::start_vm(self, vm_id).await.map_err(Into::into)
    }

    async fn stop_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::stop_vm(self, vm_id).await.map_err(Into::into)
    }

    async fn force_stop_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::force_stop_vm(self, vm_id)
            .await
            .map_err(Into::into)
    }

    async fn pause_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::pause_vm(self, vm_id).await.map_err(Into::into)
    }

    async fn resume_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::resume_vm(self, vm_id).await.map_err(Into::into)
    }

    async fn delete_vm(&self, vm_id: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::delete_vm(self, vm_id).await.map_err(Into::into)
    }

    async fn get_vm_info(
        &self,
        vm_id: &str,
    ) -> Result<crate::rpc::node::VmState, crate::vmm::VmmError> {
        VmManager::get_vm_info(self, vm_id)
            .await
            .map_err(Into::into)
    }

    async fn list_vms(&self) -> Vec<crate::rpc::node::VmState> {
        VmManager::list_vms(self).await
    }

    async fn snapshot_vm(
        &self,
        vm_id: &str,
        destination_url: &str,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::snapshot_vm(self, vm_id, destination_url)
            .await
            .map_err(Into::into)
    }

    async fn restore_vm(&self, vm_id: &str, source_url: &str) -> Result<(), crate::vmm::VmmError> {
        VmManager::restore_vm(self, vm_id, source_url)
            .await
            .map_err(Into::into)
    }

    async fn recover_vms(&self) {
        VmManager::recover_vms(self).await
    }

    fn runtime_dir(&self) -> &std::path::Path {
        VmManager::runtime_dir(self)
    }

    fn api_socket(&self, vm_id: &str) -> PathBuf {
        self.socket_path(vm_id)
    }

    async fn add_network_device(
        &self,
        vm_id: &str,
        config: &crate::rpc::node::NetConfig,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::add_network_device(self, vm_id, config)
            .await
            .map_err(Into::into)
    }

    async fn remove_network_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::remove_network_device(self, vm_id, device_id)
            .await
            .map_err(Into::into)
    }

    async fn add_disk_device(
        &self,
        vm_id: &str,
        config: &crate::rpc::node::DiskConfig,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::add_disk_device(self, vm_id, config)
            .await
            .map_err(Into::into)
    }

    async fn remove_disk_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::remove_disk_device(self, vm_id, device_id)
            .await
            .map_err(Into::into)
    }

    async fn add_device(
        &self,
        vm_id: &str,
        config: &crate::rpc::node::VfioDeviceConfig,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::add_device(self, vm_id, config)
            .await
            .map_err(Into::into)
    }

    async fn remove_device(
        &self,
        vm_id: &str,
        device_id: &str,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::remove_device(self, vm_id, device_id)
            .await
            .map_err(Into::into)
    }

    async fn resize_vm(
        &self,
        vm_id: &str,
        desired_vcpus: Option<i32>,
        desired_ram: Option<i64>,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::resize_vm(self, vm_id, desired_vcpus, desired_ram)
            .await
            .map_err(Into::into)
    }

    async fn resize_disk(
        &self,
        vm_id: &str,
        disk_id: &str,
        path: &str,
        new_size: i64,
        image_format: ProtoImageFormat,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::resize_disk(self, vm_id, disk_id, path, new_size, image_format)
            .await
            .map_err(Into::into)
    }

    async fn receive_migration(
        &self,
        vm_id: &str,
        config: ProtoVmConfig,
        port: u16,
    ) -> Result<String, crate::vmm::VmmError> {
        VmManager::receive_migration(self, vm_id, config, port)
            .await
            .map_err(Into::into)
    }

    async fn send_migration(
        &self,
        req: &ProtoSendMigrationRequest,
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::send_migration(self, req)
            .await
            .map_err(Into::into)
    }

    async fn exec_vm(
        &self,
        vm_id: &str,
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<crate::rpc::node::ExecVmResponse, crate::vmm::VmmError> {
        VmManager::exec_vm(self, vm_id, command, timeout_secs)
            .await
            .map_err(Into::into)
    }

    async fn fork_vm(
        &self,
        source_id: &str,
        forks: &[crate::rpc::node::Fork],
    ) -> Result<(), crate::vmm::VmmError> {
        self.fork(source_id, forks).await
    }

    async fn resume_forks(
        &self,
        source_id: &str,
        fork_ids: &[String],
    ) -> Result<(), crate::vmm::VmmError> {
        VmManager::resume_forks(self, source_id, fork_ids).await
    }

    async fn guest_agent_socket(&self, vm_id: &str) -> Result<PathBuf, crate::vmm::VmmError> {
        VmManager::guest_agent_socket(self, vm_id)
            .await
            .map_err(Into::into)
    }

    async fn get_vm_counters(
        &self,
        vm_id: &str,
    ) -> Result<
        std::collections::HashMap<String, std::collections::HashMap<String, i64>>,
        crate::vmm::VmmError,
    > {
        VmManager::get_vm_counters(self, vm_id)
            .await
            .map_err(Into::into)
    }

    async fn get_serial_pty_path(
        &self,
        vm_id: &str,
    ) -> Result<Option<String>, crate::vmm::VmmError> {
        VmManager::get_serial_pty_path(self, vm_id)
            .await
            .map_err(Into::into)
    }

    async fn is_vm_process_alive(&self, vm_id: &str) -> bool {
        VmManager::is_vm_process_alive(self, vm_id).await
    }
}
