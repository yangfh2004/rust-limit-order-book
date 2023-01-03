use actix_web::body::BoxBody;
use actix_web::http::header::ContentType;
use actix_web::http::StatusCode;
use actix_web::{delete, get, post, web, App, HttpResponse, HttpServer, Responder, ResponseError};

use serde::Serialize;

use std::fmt::Display;
use std::sync::Mutex;
// local module.
use order_book::account::AccountManager;
use order_book::json::{JsonAccount, JsonOrder};
use order_book::primitive::{Address, Hash};
use order_book::OrderBook;

struct AppState {
    // This shall be your database in the production env.
    // In this simple exercise, all data is stored in memory.
    manager: Mutex<AccountManager>,
    order_book: Mutex<OrderBook>,
    user_count: Mutex<u64>,
}

#[derive(Debug, Serialize)]
struct ErrNoAccount {
    address: String,
    err: String,
}

// Implement ResponseError for ErrNoAccount
impl ResponseError for ErrNoAccount {
    fn status_code(&self) -> StatusCode {
        StatusCode::NOT_FOUND
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        let body = serde_json::to_string(&self).unwrap();
        let res = HttpResponse::new(self.status_code());
        res.set_body(BoxBody::new(body))
    }
}

// Implement Display for ErrNoAccount
impl Display for ErrNoAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Serialize)]
struct ErrNoOrder {
    hash: Hash,
    err: String,
}

// Implement ResponseError for ErrNoAccount
impl ResponseError for ErrNoOrder {
    fn status_code(&self) -> StatusCode {
        StatusCode::NOT_FOUND
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        let body = serde_json::to_string(&self).unwrap();
        let res = HttpResponse::new(self.status_code());
        res.set_body(BoxBody::new(body))
    }
}

// Implement Display for ErrNoAccount
impl Display for ErrNoOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Create a new account. The username is generated internally.
#[post("/accounts")]
async fn new_account(req: web::Json<JsonAccount>, data: web::Data<AppState>) -> HttpResponse {
    let mut manager = data.manager.lock().unwrap();
    let mut count = data.user_count.lock().unwrap();
    let account = JsonAccount {
        ddxBalance: req.ddxBalance.clone(),
        usdBalance: req.usdBalance.clone(),
        traderAddress: req.traderAddress.clone(),
    };

    manager.add_json_account(format!("User {}", count).as_str(), account);
    *count += 1;

    HttpResponse::Created()
        .content_type(ContentType::plaintext())
        .insert_header(("X-Hdr", "sample"))
        .body("New account created!")
}

/// Get an account info with the corresponding trader address.
#[get("/accounts/{traderAddress}")]
#[allow(non_snake_case)]
async fn get_account(
    traderAddress: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<impl Responder, ErrNoAccount> {
    let trader: Address = traderAddress
        .parse::<Address>()
        .expect("Failed to parse trader's address!");
    let manager = data.manager.lock().unwrap();

    if let Some(account) = manager.get_json_account(&trader) {
        Ok(web::Json(account))
    } else {
        let response = ErrNoAccount {
            address: traderAddress.clone(),
            err: String::from("Account not found"),
        };
        Err(response)
    }
}

/// Delete an account with the corresponding trader address.
#[delete("/accounts/{traderAddress}")]
#[allow(non_snake_case)]
async fn delete_account(
    traderAddress: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<impl Responder, ErrNoAccount> {
    let trader: Address = traderAddress
        .parse::<Address>()
        .expect("Failed to parse trader's address!");
    let mut manager = data.manager.lock().unwrap();
    let mut count = data.user_count.lock().unwrap();

    if let Some(account) = manager.delete_account(&trader) {
        *count -= 1;
        Ok(web::Json(account))
    } else {
        let response = ErrNoAccount {
            address: traderAddress.clone(),
            err: String::from("Account not found"),
        };
        Err(response)
    }
}

/// Add an order to the order book (possibly matching other orders).
#[post("/orders")]
async fn new_order(
    req: web::Json<JsonOrder>,
    data: web::Data<AppState>,
) -> Result<impl Responder, ErrNoAccount> {
    let order = JsonOrder {
        amount: req.amount.clone(),
        nonce: req.nonce.clone(),
        price: req.price.clone(),
        side: req.side.clone(),
        traderAddress: req.traderAddress.clone(),
    };
    let mut manager = data.manager.lock().unwrap();
    let mut order_book = data.order_book.lock().unwrap();
    if let Some(fill_result) = order_book.add_order(&mut manager, order.clone()) {
        // generate json response.
        let json_res = fill_result.generate_filled_orders();
        // update accounts based the filled results.
        manager.update_accounts(fill_result);
        Ok(web::Json(json_res))
    } else {
        let response = ErrNoAccount {
            address: order.get_trader(),
            err: String::from("Account not found or account balance is not enough!"),
        };
        Err(response)
    }
}

/// Get an order info with its EIP-712 hash.
#[get("/orders/{hash}")]
async fn get_order(
    hash: web::Path<Hash>,
    data: web::Data<AppState>,
) -> Result<impl Responder, ErrNoOrder> {
    let order_hash = hash.clone();
    let order_book = data.order_book.lock().unwrap();
    match order_book.get_order(order_hash.clone()) {
        Ok(order) => Ok(web::Json(order)),
        Err(_e) => {
            let response = ErrNoOrder {
                hash: order_hash,
                err: String::from("Order not found!"),
            };
            Err(response)
        }
    }
}

/// Cancel an order info with its EIP-712 hash.
#[delete("/orders/{hash}")]
async fn cancel_order(
    hash: web::Path<Hash>,
    data: web::Data<AppState>,
) -> Result<impl Responder, ErrNoOrder> {
    let order_hash = hash.clone();
    let mut order_book = data.order_book.lock().unwrap();
    let mut manager = data.manager.lock().unwrap();
    match order_book.cancel_order(&mut manager, order_hash.clone()) {
        Ok(order) => Ok(web::Json(order)),
        Err(_e) => {
            let response = ErrNoOrder {
                hash: order_hash,
                err: String::from("Order not found"),
            };
            Err(response)
        }
    }
}

/// Get L2 order book.
#[get("/book")]
async fn get_book(data: web::Data<AppState>) -> impl Responder {
    let order_book = data.order_book.lock().unwrap();
    let l2_book = order_book.generate_l2_order_book();
    web::Json(l2_book)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        manager: Mutex::new(AccountManager::new()),
        order_book: Mutex::new(OrderBook::new("DDX".to_string())),
        user_count: Mutex::new(0),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .service(new_account)
            .service(get_account)
            .service(delete_account)
            .service(new_order)
            .service(get_order)
            .service(cancel_order)
            .service(get_book)
    })
    .bind(("127.0.0.1", 4321))?
    .run()
    .await
}
