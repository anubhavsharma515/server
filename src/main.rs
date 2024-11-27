use kv::{Bucket, Config, Store, Codec};
use axum::{
    extract::{Json, Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::Arc,
};

use tokio::{runtime::Builder, sync::Mutex};

// Define the Song struct with serialization and deserialization
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Song {
    id: usize,
    title: String,
    artist: String,
    genre: String,
    play_count: usize,
}

// This is the JSON response that will be returned when a song is
// added to the catalog
#[derive(Debug, Deserialize)]
struct NewSong {
    title: String,
    artist: String,
    genre: String,
}

// These are the valid params that can be passed to search from
// songs within the catalog
#[derive(Debug, Deserialize)]
struct QueryParams {
    title: Option<String>,
    artist: Option<String>,
    genre: Option<String>,
}

// State to be shared across the requesting threads
// State should be sharing context of a database to read/write
#[derive(Clone)]
struct AppState<'a> {
    store: Bucket<'a, kv::Integer, kv::Json<Song>>,
    visit_count: Arc<Mutex<usize>>,
    refresh: Arc<Mutex<bool>>,
}

#[tokio::main]
async fn main() {
    let runtime = Builder::new_multi_thread()
        .worker_threads(8) // specify the number of threads you want
        .build()
        .unwrap();

    let cfg = Config::new("server_db"); // new data store that writes to disk
    let db = Store::new(cfg).unwrap(); // store can be thought of as the DB
    // Key-val store, where key is an int and val is of type Json<Song>
    // bucket can be thought of as a table
    let store = db.bucket::<kv::Integer, kv::Json<Song>>(Some("songs")).unwrap();

    let visit_count = Arc::new(Mutex::new(0));
    let refresh = Arc::new(Mutex::new(false));
    let state = AppState { store, visit_count, refresh };

    // Spawn a background task for syncing the store when marked dirty
    let refresh_clone = state.refresh.clone();
    let store_clone = state.store.clone();

    tokio::spawn(async move {
        loop {
            let mut sync = false;
            {
                let mut should_refresh = refresh_clone.lock().await;
                if *should_refresh && store_clone.len() > 1000 {
                    sync = true;
                    *should_refresh = false;
                }
            }

            if sync {
                store_clone.flush_async().await.unwrap();
            }
        }
    });

    // Define the address
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    // Define the app with routes and shared state
    let app = Router::new()
        .route("/", get(|| async { "Welcome to the web server!" }))
        .route("/count", get(handle_count))
        .route("/songs/new", post(add_new_song))
        .route("/songs/play/:id", get(play_song))
        .route("/songs/search", get(search_song))
        .with_state(state);

    // Print the message only after the server successfully binds
    println!("Server is running at http://{}", addr);

    // Start the server
    runtime.spawn(async move {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    }).await.unwrap();
}

async fn handle_count(State(state): State<AppState<'_>>) -> String {

    let mut vc = state.visit_count.lock().await;
    *vc += 1;
    format!("Visit count: {}", *vc)
}

// Handler to add a new song
async fn add_new_song(
    State(state): State<AppState<'_>>,
    Json(payload): Json<NewSong>,
) -> Json<Song> {
    let store = &state.store;

    // Generate an ID for the new song
    let id = store.len() + 1;
    let song = Song {
        id,
        title: payload.title,
        artist: payload.artist,
        genre: payload.genre,
        play_count: 0,
    };

    // Add the new song to the store
    store
        .set(&kv::Integer::from(id), &kv::Json(song.clone()))
        .unwrap();

    // Mark the store as dirty
    let mut refresh = state.refresh.lock().await;
    *refresh = true;

    Json(song)
}

// Handler to search songs based on query parameters
async fn search_song(
    State(state): State<AppState<'_>>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<Song>> {
    let store = &state.store;

    // Collect matching songs
    let results = store
        .iter()
        .filter_map(|item| {
            let song = match item {
                Ok(item) => match item.value::<kv::Json<Song>>() {
                    Ok(song) => song.into_inner(),
                    Err(_) => return None,
                },
                Err(_) => return None,
            };

            // Check if the song matches the query
            if let Some(title) = &params.title {
                if !song.title.to_lowercase().contains(&title.to_lowercase()) {
                    return None;
                }
            }
            if let Some(artist) = &params.artist {
                if !song.artist.to_lowercase().contains(&artist.to_lowercase()) {
                    return None;
                }
            }
            if let Some(genre) = &params.genre {
                if !song.genre.to_lowercase().contains(&genre.to_lowercase()) {
                    return None;
                }
            }
            Some(song)
        })
        .collect();

    Json(results)
}

// Handler to play a song by ID
async fn play_song(
    State(state): State<AppState<'_>>,
    Path(id): Path<usize>,
) -> Json<serde_json::Value> {
    let store = &state.store;
    let key = kv::Integer::from(id);

    // Find and update the song's play count
    let updated_song = match store.get(&key).unwrap() {
        Some(value) => {
            let mut song: Song = value.into_inner();
            song.play_count += 1;

            // Update the store
            store.set(&key, &kv::Json(song.clone())).unwrap();
            Some(song)
        }
        None => None,
    };

    match updated_song {
        Some(song) => Json(serde_json::json!(song)),
        None => Json(serde_json::json!({ "error": "Song not found" })),
    }
}
