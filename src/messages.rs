use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ConfigurationClient {pub client: String,}
#[derive(Deserialize, Debug)]
pub struct ConfigurationMinUploadSpeed {pub min_upload_speed: u16,}
#[derive(Deserialize, Debug)]
pub struct ConfigurationMaxUploadSpeed {pub max_upload_speed: u16,}
