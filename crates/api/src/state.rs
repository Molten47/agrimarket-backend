use sqlx::PgPool;
use redis::aio::ConnectionManager;
use app_core::config::AppConfig;
use auth::service::AuthService;
use ws::WsBroadcaster;
use mailer::MailerService;

pub struct AppState {
    pub config:      AppConfig,
    pub db:          PgPool,
    pub redis:       ConnectionManager,
    pub auth:        AuthService,
    pub broadcaster: WsBroadcaster,
    pub mailer: MailerService
}

impl AppState {
    pub fn new(
        config:      AppConfig,
        db:          PgPool,
        redis:       ConnectionManager,
        auth:        AuthService,
        broadcaster: WsBroadcaster,
        mailer: MailerService
    ) -> Self {
        Self { config, db, redis, auth, broadcaster, mailer }
    }

}