use crate::errors::{Error, ErrorCode};
use std::sync::Arc;

use crate::context::Context;

pub async fn fetch_beatmap_osu_file(
    beatmap_id: i32,
    context: Arc<Context>,
) -> Result<Vec<u8>, Error> {
    let base_url = &context.config.beatmaps_service_base_url;
    let url = format!("{base_url}/api/osu-api/v1/osu-files/{beatmap_id}");
    match reqwest::get(&url).await {
        Ok(response) => match response.error_for_status() {
            Ok(valid_response) => match valid_response.bytes().await {
                Ok(bytes) => Ok(bytes.to_vec()),
                Err(e) => {
                    log::error!("Failed to read response bytes {:?}", e);
                    Err(Error {
                        error_code: ErrorCode::DependencyFailed,
                        user_feedback: "Failed to read response bytes",
                    })
                }
            },
            Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => Err(Error {
                error_code: ErrorCode::NotFound,
                user_feedback: "Beatmap not found",
            }),
            Err(e) => {
                log::error!("Failed to fetch beatmap osu file {:?}", e);
                Err(Error {
                    error_code: ErrorCode::DependencyFailed,
                    user_feedback: "Failed to fetch beatmap osu file",
                })
            }
        },
        Err(e) => {
            log::error!("Network error while fetching beatmap {:?}", e);
            Err(Error {
                error_code: ErrorCode::DependencyFailed,
                user_feedback: "Network error while fetching beatmap",
            })
        }
    }
}
