//! Cluster-voter namespace DDL (`007` protocol mapping).

#[async_trait::async_trait]
pub trait NamespaceControl: Send + Sync {
    async fn exists(&self, name: &str) -> Result<bool, String>;
    async fn create(&self, name: &str) -> Result<(), String>;
    async fn drop_namespace(&self, name: &str) -> Result<(), String>;
    async fn rename(&self, name: &str, new_name: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct PermissiveNamespaceControl;

#[async_trait::async_trait]
impl NamespaceControl for PermissiveNamespaceControl {
    async fn exists(&self, _name: &str) -> Result<bool, String> {
        Ok(true)
    }

    async fn create(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    async fn drop_namespace(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    async fn rename(&self, _name: &str, _new_name: &str) -> Result<(), String> {
        Ok(())
    }
}
