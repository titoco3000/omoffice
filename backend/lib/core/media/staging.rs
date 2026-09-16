//! Each user gets one staging slot - temporary storage for presigned uploads.
//! They need to be promoted to something else by other modules.

use uuid::Uuid;

use super::storage::{MediaError, MediaState};

/// Where `user_id`'s one staged upload stays before being consumed.
fn staging_key(user_id: &str) -> String {
    format!("uploads/{user_id}")
}

impl MediaState {
    pub(crate) async fn create_upload_link(
        &self,
        user_id: &str,
        content_type: &str,
    ) -> Result<String, MediaError> {
        self.presign_put(&staging_key(user_id), content_type).await
    }

    pub(crate) async fn get_staged_content_type(
        &self,
        user_id: &str,
    ) -> Result<String, MediaError> {
        self.head_content_type(&staging_key(user_id)).await
    }

    pub(crate) async fn get_staged(&self, user_id: &str) -> Result<(Vec<u8>, String), MediaError> {
        self.fetch(&staging_key(user_id)).await
    }

    pub(crate) async fn clear_staged(&self, user_id: &str) {
        self.delete(&staging_key(user_id)).await
    }

    /// For simple upgrades, just moves to the new folder
    #[allow(dead_code)]
    pub(crate) async fn upgrade(
        &self,
        user_id: &str,
        new_folder: &str,
    ) -> Result<String, MediaError> {
        let staging_key = staging_key(user_id);
        let final_key = format!("{new_folder}/{}", Uuid::new_v4());
        self.copy(&staging_key, &final_key).await?;
        self.delete(&staging_key).await;
        Ok(final_key)
    }
}
