use crate::models::volumes::{NewVolume, VolumeError, VolumeName};

use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewVolumeRequest {
    pub name: String,
    pub size: u64,
    pub url: Option<String>,
}

impl TryFrom<NewVolumeRequest> for NewVolume {
    type Error = VolumeError;

    fn try_from(value: NewVolumeRequest) -> Result<Self, Self::Error> {
        let name = VolumeName::new(value.name)?;
        if value.size <= 0 {
            return Err(VolumeError::InvalidSize(value.size.to_string()));
        }

        Ok(Self {
            name,
            size: value.size as i64,
            url: value.url,
        })
    }
}
