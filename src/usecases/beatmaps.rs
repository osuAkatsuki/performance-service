use std::sync::Arc;

// use s3::error::S3Error;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    _beatmap_md5: &str,
    _context: Arc<Context>,
) -> anyhow::Result<Vec<u8>> {
    // let beatmap_path = &format!("beatmaps/{beatmap_id}.osu");

    // let existing_file = match context.bucket.get_object(beatmap_path).await {
    //     Ok(existing_file) => Ok(Some(existing_file)),
    //     Err(S3Error::Http(status_code, _)) if status_code == 404 => Ok(None),
    //     Err(e) => Err(e),
    // }?;

    // match existing_file {
    //     Some(existing_file) => {
    //         let osu_file_data = existing_file.to_vec();
    //         let osu_file_data_md5 = format!("{:x}", md5::compute(&osu_file_data));

    //         if osu_file_data_md5 == beatmap_md5 {
    //             return Ok(osu_file_data);
    //         }
    //     }
    //     None => {}
    // }

    let osu_response = reqwest::get(&format!("https://old.ppy.sh/osu/{beatmap_id}"))
        .await?
        .error_for_status()?;

    let response_bytes = osu_response.bytes().await?.to_vec();

    // context
    //     .bucket
    //     .put_object(beatmap_path, &response_bytes)
    //     .await?;

    Ok(response_bytes)
}
