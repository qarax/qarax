use crate::models::volumes::{NewVolume, VolumeError, VolumeName, VolumeType};

use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewVolumeRequest {
    pub name: String,
    pub size: Option<i64>,
    pub url: Option<String>,
}

impl TryFrom<NewVolumeRequest> for NewVolume {
    type Error = VolumeError;

    fn try_from(value: NewVolumeRequest) -> Result<Self, Self::Error> {
        let name = VolumeName::new(value.name)?;
        if let Some(size) = value.size {
            if size <= 0 {
                return Err(VolumeError::InvalidSize(size.to_string()));
            }
        }

        Ok(Self {
            name,
            size: value.size,
            url: value.url,
            volume_type: VolumeType::Drive,
        })
    }
}
