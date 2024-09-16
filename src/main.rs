use axum::{
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode},
    response::Html,
    routing::{delete, get},
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, prelude::FromRow};
use std::{net::SocketAddr, time::Duration};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, FromRow)]
struct Podcast {
    id: Uuid,
    title: String,
    url: String,
    created_at: Option<chrono::NaiveDateTime>,
}

mod service {
    use super::Podcast;
    use axum::{
        body::Bytes,
        extract::{Multipart, Path, State},
        http::StatusCode,
        response::Json,
    };
    use sqlx::PgPool;
    use tokio::io::AsyncWriteExt;
    use uuid::Uuid;

    pub async fn list_podcasts(State(pool): State<PgPool>) -> Json<Vec<Podcast>> {
        let podcasts = sqlx::query_as!(Podcast, "SELECT * FROM podcast")
            .fetch_all(&pool)
            .await
            .expect("Failed to fetch podcasts");

        Json(podcasts)
    }

    /// Create a new podcast
    /// This function expects a multipart request with the following fields:
    /// - title: String
    /// - file: File
    /// The file field should be a valid audio file.
    /// The function will save the file to the disk and store the metadata in the database.
    pub async fn create_podcast(
        State(pool): State<PgPool>,
        mut payload: Multipart,
    ) -> Result<Json<Option<Podcast>>, StatusCode> {
        let mut title = None;
        let mut url = None;

        let mut file: Option<tokio::fs::File> = None;
        let mut bytes: Option<Bytes> = None;

        while let Some(field) = payload.next_field().await.unwrap() {
            let name = field.name().unwrap();
            match name {
                "title" => {
                    title = Some(field.text().await.unwrap());
                }
                "file" => {
                    let content_type = field.content_type().unwrap().to_string();
                    let audio_content_types = vec![
                        "audio/mpeg",
                        "audio/mp3",
                        "audio/ogg",
                        "audio/wav",
                        "audio/flac",
                    ];

                    if !audio_content_types.contains(&content_type.as_str()) {
                        return Err(StatusCode::BAD_REQUEST);
                    }

                    let file_name = format!(
                        "{}.{}",
                        Uuid::new_v4(),
                        content_type.split("/").last().unwrap()
                    );

                    let file_path = format!("./media/audio/{}", file_name);
                    file = Some(
                        tokio::fs::File::create(&file_path)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    );

                    bytes = Some(
                        field
                            .bytes()
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    );

                    url = Some(format!("/audio/{}", file_name));
                }
                _ => {}
            }
        }

        let title = title.unwrap();
        let url = url.unwrap();

        if let Some(mut file) = file {
            if let Some(bytes) = bytes {
                file.write_all(&bytes)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }

        let podcast = sqlx::query_as!(
            Podcast,
            "INSERT INTO podcast (title, url) VALUES ($1, $2) RETURNING *",
            title,
            url
        )
        .fetch_one(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(Json(Some(podcast)))
    }

    pub async fn get_podcast(
        Path(id): Path<Uuid>,
        State(pool): State<PgPool>,
    ) -> Result<Json<Podcast>, StatusCode> {
        let podcast = sqlx::query_as!(Podcast, "SELECT * FROM podcast WHERE id = $1", id)
            .fetch_one(&pool)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;

        Ok(Json(podcast))
    }

    pub async fn delete_podcast(
        Path(id): Path<Uuid>,
        State(pool): State<PgPool>,
    ) -> Result<StatusCode, StatusCode> {
        // Delete the file from the disk
        let url: String = sqlx::query_scalar!("SELECT url FROM podcast WHERE id = $1", id)
            .fetch_one(&pool)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;

        let file_path = format!("./media{}", url);
        tokio::fs::remove_file(&file_path)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        sqlx::query!("DELETE FROM podcast WHERE id = $1", id)
            .execute(&pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(StatusCode::NO_CONTENT)
    }

    pub async fn delete_all_podcasts(State(pool): State<PgPool>) -> Result<StatusCode, StatusCode> {
        // Delete all files from the disk
        let urls: Vec<String> = sqlx::query_scalar!("SELECT url FROM podcast")
            .fetch_all(&pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for url in urls {
            let file_path = format!("./media{}", url);
            tokio::fs::remove_file(&file_path)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }

        sqlx::query!("DELETE FROM podcast")
            .execute(&pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(StatusCode::NO_CONTENT)
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_connection_string = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/podcast".to_string());
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&db_connection_string)
        .await
        .expect("Failed to connect to Postgres");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    // create media directory
    tokio::fs::create_dir_all("./media/audio")
        .await
        .expect("Failed to create media directory");

    let app_router = Router::new()
        .nest_service("/assets", ServeDir::new("./static/dist/assets"))
        .route_service("/", ServeFile::new("./static/dist/index.html"))
        .route("/health_check", get(health_check))
        .route(
            "/podcasts",
            get(service::list_podcasts).post(service::create_podcast),
        )
        .route(
            "/podcasts/:id",
            get(service::get_podcast).delete(service::delete_podcast),
        )
        .route("/podcasts", delete(service::delete_all_podcasts))
        .fallback(not_found)
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(
            1024 * 1024 * 250, // 250 MB
        ))
        .layer(
            CorsLayer::new()
                .allow_origin("http://0.0.0.0:3000".parse::<HeaderValue>().unwrap())
                .allow_methods(vec![Method::GET, Method::POST, Method::DELETE]),
        )
        .with_state(pool);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::debug!("Listening on: {}", listener.local_addr().unwrap());
    axum::serve(listener, app_router.layer(TraceLayer::new_for_http()))
        .await
        .unwrap();
}

async fn health_check() -> Html<&'static str> {
    Html("<h1>Service is running</h1>")
}

async fn not_found() -> (StatusCode, Html<&'static str>) {
    (StatusCode::NOT_FOUND, Html("<h1>Not Found</h1>"))
}
