use std::sync::Arc;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    context: Arc<Context>,
) -> anyhow::Result<Vec<u8>> {
    let beatmap_path = &format!("beatmaps/{beatmap_id}.osu");

    let existing_file = context.bucket.get_object(beatmap_path).await?;
    if existing_file.status_code() == 200 {
        return Ok(existing_file.as_slice().to_vec());
    }

    let osu_response = reqwest::get(&format!("https://old.ppy.sh/osu/{beatmap_id}"))
        .await?
        .error_for_status()?;

    let response_bytes = osu_response.bytes().await?.to_vec();

    context
        .bucket
        .put_object(beatmap_path, &response_bytes)
        .await?;

    Ok(response_bytes)
}
