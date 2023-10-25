use std::sync::Arc;

use s3::error::S3Error;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    context: Arc<Context>,
) -> anyhow::Result<Vec<u8>> {
    let beatmap_path = &format!("beatmaps/{beatmap_id}.osu");

    let existing_file = match context.bucket.get_object(beatmap_path).await {
        Ok(existing_file) => Ok(Some(existing_file)),
        Err(S3Error::Http(status_code, _)) if status_code == 404 => Ok(None),
        Err(e) => Err(e),
    }?;
    if existing_file.is_some() {
        return Ok(existing_file.unwrap().to_vec());
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
