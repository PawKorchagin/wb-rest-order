use axum::{
    extract::{Json, State},
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};

use serde::{Deserialize, Serialize, Deserializer};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::env;
use dotenv::dotenv;

use log::{debug, error};
use log4rs;

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Delivery {
    name: String,
    phone: String,
    zip: String,
    city: String,
    address: String,
    region: String,
    email: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Payment {
    transaction: String,
    request_id: String,
    currency: String,
    provider: String,
    amount: u32,
    payment_dt: u64,
    bank: String,
    delivery_cost: u32,
    goods_total: u32,
    custom_fee: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Item {
    chrt_id: u64,
    track_number: String,
    price: u32,
    rid: String,
    name: String,
    sale: u32,
    size: String,
    total_price: u32,
    nm_id: u64,
    brand: String,
    status: u32,
}

use time::OffsetDateTime;

fn deserialize_offset_datetime<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc3339)
        .map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Order {
    order_uid: String,
    track_number: String,
    entry: String,
    delivery: Delivery,
    payment: Payment,
    items: Vec<Item>,
    locale: String,
    internal_signature: String,
    customer_id: String,
    delivery_service: String,
    shardkey: String,
    sm_id: u32,
    #[serde(deserialize_with = "deserialize_offset_datetime")]
    date_created: OffsetDateTime,
    oof_shard: String,
}

use axum::extract::FromRef;


#[derive(Clone)]
struct AppState {
    shared_state: Arc<Mutex<Option<Order>>>,
    db_pool: Arc<Pool<Postgres>>,
}

impl FromRef<AppState> for Arc<Mutex<Option<Order>>> {
    fn from_ref(app_state: &AppState) -> Arc<Mutex<Option<Order>>> {
        app_state.shared_state.clone()
    }
}

impl FromRef<AppState> for Arc<Pool<Postgres>> {
    fn from_ref(app_state: &AppState) -> Arc<Pool<Postgres>> {
        app_state.db_pool.clone()
    }
}


// Define shared state
type SharedState = Arc<Mutex<Option<Order>>>;

async fn init_orders_shema(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let create_table_sql = "
        CREATE TABLE IF NOT EXISTS orders (
            id SERIAL PRIMARY KEY,
            order_uid VARCHAR NOT NULL,
            track_number VARCHAR NOT NULL,
            entry VARCHAR NOT NULL,
            delivery JSONB,
            payment JSONB,
            items JSONB,
            locale VARCHAR NOT NULL,
            internal_signature VARCHAR NOT NULL,
            customer_id VARCHAR NOT NULL,
            delivery_service VARCHAR NOT NULL,
            shardkey VARCHAR NOT NULL,
            sm_id VARCHAR NOT NULL,
            date_created TIMESTAMPTZ DEFAULT now(),
            oof_shard INTEGER
        );
    ";

    sqlx::query(create_table_sql).execute(pool).await?;

    Ok(())
}

async fn save_order_to_db(pool: &Pool<Postgres>, order: &Order) -> Result<(), sqlx::Error> {
    let delivery_json = serde_json::to_value(&order.delivery).unwrap();
    let payment_json = serde_json::to_value(&order.payment).unwrap();
    let items_json = serde_json::to_value(&order.items).unwrap();

    sqlx::query!(
        r#"
        INSERT INTO orders (
            order_uid, track_number, entry, delivery, payment, items, locale,
            internal_signature, customer_id, delivery_service, shardkey, sm_id,
            date_created, oof_shard
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        "#,
        order.order_uid,
        order.track_number,
        order.entry,
        delivery_json,
        payment_json,
        items_json,
        order.locale,
        order.internal_signature,
        order.customer_id,
        order.delivery_service,
        order.shardkey,
        order.sm_id as i32,
        order.date_created,
        order.oof_shard
    )
    .execute(pool)
    .await?;

    Ok(())
}

// Handler to receive the order
async fn handle_order(
    State(app_state): State<AppState>,
    Json(order): Json<Order>,
) -> impl IntoResponse {
    // Access the shared state
    let mut state = app_state.shared_state.lock().await;
    *state = Some(order.clone());

    // Save the order to the database
    if let Err(e) = save_order_to_db(&app_state.db_pool, &order).await {
        eprintln!("Failed to save order: {:?}", e);
        return "Failed to save order".into_response();
    }

    println!("Received order: {:?}", order);
    "Order received".into_response()
}

async fn show_order(State(app_state): State<AppState>) -> impl IntoResponse {
    let state = app_state.shared_state.lock().await;
    if let Some(order) = &*state {
        let pretty_json = serde_json::to_string_pretty(order).unwrap();
        Html(format!("<pre>{}</pre>", pretty_json))
    } else {
        Html("<p>No order received yet.</p>".to_string())
    }
}

#[tokio::main]
async fn main() {
    log4rs::init_file("src/resources/log4rs.yaml", Default::default()).unwrap();

    dotenv().ok();
    
    for (key, value) in env::vars() {
        println!("{}: {}", key, value);
    }
    
    // Initialize shared state
    let shared_state = Arc::new(Mutex::new(None));
    
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    
    let pool = PgPoolOptions::new()
        .connect(&db_url)
        .await
        .expect("Failed to create pool");

    if let Err(e) = init_orders_shema(&pool).await {
        eprintln!("Failed to create table orders: {}", e);
    }

    let app_state = AppState {
        shared_state: shared_state.clone(),
        db_pool: Arc::new(pool),
    };

    // Build the application via routes 
    // 1. get order in pretty format
    // 2. accept post-request
    let app = Router::new()
        .route("/", get(show_order))
        .route("/order", post(handle_order))
        .with_state(app_state);

    // Specify the address to run the server on
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {}", addr);

    debug!("log check");


    // Run the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
