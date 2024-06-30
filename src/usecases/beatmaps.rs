use std::sync::Arc;

use s3::error::S3Error;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    beatmap_md5: &str,
    context: Arc<Context>,
) -> anyhow::Result<Vec<u8>> {
    let beatmap_path = &format!("beatmaps/{beatmap_id}.osu");

    // TODO: rethink this caching by:
    // 1. moving this .osu file update logic to beatmaps-service
    // 2. marking a "last updated" date on files in s3
    // 3. checking if the file in s3 is older than the last updated date
    // 4. if it is, updating both the .osu file as well as the beatmaps in db
    // 5. ensuring all other updates in the ecosystem are updating both the .osu file and the beatmaps in db
    let existing_file = match context.bucket.get_object(beatmap_path).await {
        Ok(existing_file) => Ok(Some(existing_file)),
        Err(S3Error::Http(status_code, _)) if status_code == 404 => Ok(None),
        Err(e) => Err(e),
    }?;

    match existing_file {
        Some(existing_file) => {
            let osu_file_data = existing_file.to_vec();
            let osu_file_data_md5 = format!("{:x}", md5::compute(&osu_file_data));

            if osu_file_data_md5 == beatmap_md5 {
                return Ok(osu_file_data);
            }
        }
        None => {}
    }

    let base_url = &context.config.beatmaps_service_base_url;
    let url = &format!("{base_url}/api/osu-api/v1/osu-files/{beatmap_id}");
    let osu_response = reqwest::get(url).await?.error_for_status()?;

    let response_bytes = osu_response.bytes().await?.to_vec();

    context
        .bucket
        .put_object(beatmap_path, &response_bytes)
        .await?;

    Ok(response_bytes)
}
