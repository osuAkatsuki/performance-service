use std::sync::Arc;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    context: Arc<Context>,
) -> anyhow::Result<Vec<u8>> {
    let base_url = &context.config.beatmaps_service_base_url;
    let url = &format!("{base_url}/api/osu-api/v1/osu-files/{beatmap_id}");
    let osu_response = reqwest::get(url).await?.error_for_status()?;

    let response_bytes = osu_response.bytes().await?.to_vec();

    Ok(response_bytes)
}
