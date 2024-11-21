use axum::{
    extract::State,
    routing::get,
    Router
};

// Arc is a thread-safe pointer to shared data. 
// It provides shared ownership to something on heap.
// Mutex is for protecting shared data
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;


#[tokio::main]
async fn main() {
    // Create a shared counter with Arc and Mutex
    let visit_count = Arc::new(Mutex::new(0));

    // Create the app with the count route
    let app = Router::new()
        .route("/", get(|| async { "Welcome to the web server!" }))
        .route("/count", get(handle_count))
        .with_state(visit_count.clone()); // Ensure `Arc` is cloned and shared properly

    // Define the address
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Server is running at http://{}", addr);

    // Start the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

// Handler for the "/count" route
async fn handle_count(State(visit_count): State<Arc<Mutex<usize>>>) -> String {
    // Increment the counter
    let mut count = visit_count.lock().unwrap();
    *count += 1;

    // Return the current count
    format!("Visit count: {}", *count)
}
