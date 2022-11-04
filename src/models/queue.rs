#[derive(
    serde::Serialize, serde::Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
#[archive(compare(PartialEq))]
#[archive_attr(derive(bytecheck::CheckBytes))]
pub struct QueueRequest {
    pub user_id: i32,
    pub rework_id: i32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueueResponse {
    pub success: bool,
    pub message: Option<String>,
}
