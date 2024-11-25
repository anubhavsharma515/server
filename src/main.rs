use axum::{
    extract::{Json, Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use num_cpus;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Read,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::task;

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
#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppState {
    songs: HashMap<usize, Song>,  // Change Vec to HashMap
    next_id: usize,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            songs: HashMap::new(),
            next_id: 1,
        }
    }
}

const FILE_PATH: &str = "songs.json";

// The background task that saves state to the file asynchronously
async fn save_state_to_file_async(state: AppState) {
    let content = match serde_json::to_string_pretty(&state) {
        Ok(json) => json,
        Err(_) => {
            eprintln!("Error serializing state. Changes will not be saved.");
            return;
        }
    };

    // Write to the file asynchronously without blocking the main thread
    if let Err(_) = tokio::fs::write(FILE_PATH, content).await {
        eprintln!("Error writing state to file.");
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let state = Arc::new(Mutex::new(load_state_from_file("songs.json")));

    // Define the address
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    let server = axum::Server::try_bind(&addr).unwrap_or_else(|err| {
        eprintln!("Failed to bind to {}: {}", addr, err);
        std::process::exit(1);
    });

    // Print the message only after the server successfully binds
    println!("Server is running at http://{}", addr);

    // Create the app
    let app = Router::new()
        .route("/", get(|| async { "Welcome to the web server!" }))
        .route("/songs/new", post(add_new_song))
        .route("/songs/play/:id", get(play_song))
        .route("/songs/search", get(search_song))
        .with_state(state.clone()); // Share the state

    // Start the server
    server 
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn add_new_song(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(payload): Json<NewSong>,
) -> Json<Song> {
    // Lock the state only for inserting the new song
    let mut state = state.lock().unwrap();

    // Add the new song
    let new_song = Song {
        id: state.next_id,
        title: payload.title.clone(),
        artist: payload.artist.clone(),
        genre: payload.genre.clone(),
        play_count: 0,
    };

    state.songs.insert(new_song.id, new_song.clone()); // Insert into HashMap
    state.next_id += 1; // Increment the ID counter

    // Offload the file save task to a background task
    let state_clone = state.clone();
    // tokio::spawn(save_state_to_file_async(state_clone));

    // Return the newly added song as JSON
    Json(new_song)
}

async fn search_song(
    State(state): State<Arc<Mutex<AppState>>>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<Song>> {
    let state = state.lock().unwrap();

    let results: Vec<Song> = state
        .songs
        .values()
        .filter(|song| {
            params.title.as_ref().map_or(true, |t| {
                song.title.to_lowercase().contains(&t.to_lowercase())
            }) && params.artist.as_ref().map_or(true, |a| {
                song.artist.to_lowercase().contains(&a.to_lowercase())
            }) && params.genre.as_ref().map_or(true, |g| {
                song.genre.to_lowercase().contains(&g.to_lowercase())
            })
        })
        .cloned()
        .collect();

    Json(results)
}

async fn play_song(
    State(state): State<Arc<Mutex<AppState>>>,
    Path(id): Path<usize>,
) -> Json<serde_json::Value> {
    let updated_song = {
        let mut state = state.lock().unwrap(); // Lock the state to modify it

        if let Some(song) = state.songs.get_mut(&id) { // Use `get_mut` for HashMap
            song.play_count += 1; // Increment play count
            Some(song.clone()) // Clone the updated song for saving and returning
        } else {
            return Json(serde_json::json!({ "error": "Song not found" })); // Handle song not found
        }
    }; // The `MutexGuard` is dropped here

    // Offload the file save task to a background task
    {
        let state = state.lock().unwrap();
        tokio::spawn(save_state_to_file_async(state.clone()));
    }

    Json(serde_json::json!(updated_song.unwrap()))
}

fn load_state_from_file(file_path: &str) -> AppState {
    let mut file = match OpenOptions::new().read(true).open(file_path) {
        Ok(file) => file,
        Err(_) => return AppState::default(), // Return default state if the file doesn't exist
    };

    let mut content = String::new();
    if let Err(_) = file.read_to_string(&mut content) {
        eprintln!("Error reading state file. Starting with default state.");
        return AppState::default();
    }

    match serde_json::from_str(&content) {
        Ok(state) => state,
        Err(_) => {
            eprintln!("Error deserializing state file. Starting with default state.");
            AppState::default()
        }
    }
}
