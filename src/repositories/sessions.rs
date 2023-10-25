use crate::context::Context;
use redis::AsyncCommands;
use std::sync::Arc;
use uuid::Uuid;

pub struct SessionsRepository {
    context: Arc<Context>,
}

impl SessionsRepository {
    pub fn new(context: Arc<Context>) -> Self {
        Self { context }
    }

    pub async fn create(&self, user_id: i32) -> anyhow::Result<String> {
        let mut redis_conn = self.context.redis.get_async_connection().await?;
        let mut session_token: Option<String> = redis_conn
            .get(format!("rework:sessions:ids:{}", user_id))
            .await?;

        if session_token.is_none() {
            session_token = Some(Uuid::new_v4().to_string());

            let _: () = redis_conn
                .set_ex(
                    format!("rework:sessions:ids:{}", user_id),
                    session_token.clone().unwrap(),
                    3600 * 2, // 2 hours
                )
                .await?;

            let _: () = redis_conn
                .set_ex(
                    format!("rework:sessions:{}", session_token.clone().unwrap()),
                    user_id,
                    3600 * 2, // 2 hours
                )
                .await?;
        }

        Ok(session_token.unwrap())
    }

    pub async fn delete(&self, session_token: String) -> anyhow::Result<()> {
        let mut connection = self.context.redis.get_async_connection().await?;
        let user_id: Option<i32> = connection
            .get(format!("rework:sessions:{}", session_token))
            .await?;

        if user_id.is_none() {
            return Ok(());
        }

        let user_id = user_id.unwrap();

        let _: () = connection
            .del(format!("rework:sessions:{}", session_token))
            .await?;

        let _: () = connection
            .del(format!("rework:sessions:ids:{}", user_id))
            .await?;

        Ok(())
    }
}
