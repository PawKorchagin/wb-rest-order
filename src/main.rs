mod config;
mod db;
mod routes;

use axum::Router;
use db::init_db;
use routes::create_routes;

#[tokio::main]
async fn main() {
    let pool = init_db().await;

    let app = create_routes(pool);

    let addr = "127.0.0.1:3000".parse().unwrap();
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
