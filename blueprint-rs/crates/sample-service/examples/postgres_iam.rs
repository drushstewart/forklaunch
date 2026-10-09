//! Run with a restricted `DATABASE_URL` and a trusted `TENANT_ID`.
//! Apply migrations separately: use an owner `DATABASE_URL` and pass --migrate.
use forklaunch_blueprint_sample_service::async_iam::session_iam_router;
use forklaunch_core::persistence::TenantContext;
use forklaunch_iam_base::postgres::PostgresIamService;
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if !arguments.is_empty() && arguments != ["--migrate"] {
        return Err("Expected no arguments or --migrate".into());
    }
    let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .map_err(|_| "Unable to connect to PostgreSQL")?;
    if arguments == ["--migrate"] {
        PostgresIamService::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let tenant =
        TenantContext::new(std::env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let service = Arc::new(PostgresIamService::new(pool.clone(), tenant).await?);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_owned());
    let address: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("IAM listening on http://{}", listener.local_addr()?);
    axum::serve(listener, session_iam_router(service).into_router())
        .with_graceful_shutdown(shutdown())
        .await?;
    pool.close().await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = signal.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
