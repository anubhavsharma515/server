use axum::{
    extract::{Json, Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

// Arc is a thread-safe pointer to shared data.
// It provides shared ownership to something on heap.
// Mutex is for protecting shared data
use std::{
    fs::{self, OpenOptions},
    io::Read,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

// Define the Song struct with serialization and deserialization
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Song {
    id: usize,
    title: String,
    artist: String,
    genre: String,
    play_count: usize,
}

#[derive(Debug, Deserialize)]
struct NewSong {
    title: String,
    artist: String,
    genre: String,
}

#[derive(Debug, Deserialize)]
struct QueryParams {
    title: Option<String>,
    artist: Option<String>,
    genre: Option<String>,
}

// State to be shared across the requesting threads
#[derive(Debug, Serialize, Deserialize, Default)]
struct AppState {
    songs: Vec<Song>,
    next_id: usize,
}

#[tokio::main]
async fn main() {
    // Create a shared counter with Arc and Mutex
    let state = Arc::new(Mutex::new(load_state_from_file("songs.json")));

    // Create the app with the count route
    let app = Router::new()
        .route("/", get(|| async { "Welcome to the web server!" }))
        .route("/songs/new", post(add_new_song))
        .route("/songs/play/:id", get(play_song))
        .route("/songs/search", get(search_song))
        .with_state(state.clone()); // Share the state

    // Define the address
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Server is running at http://{}", addr);

    // Start the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    save_state_to_file(&state.lock().unwrap(), "songs.json");
}

async fn add_new_song(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(payload): Json<NewSong>,
) -> Json<Song> {
    // Lock the state and add the new song
    let mut state = state.lock().unwrap();

    let new_song = Song {
        id: state.next_id,
        title: payload.title,
        artist: payload.artist,
        genre: payload.genre,
        play_count: 0,
    };

    state.songs.push(new_song.clone());
    state.next_id += 1; // Increment the ID counter

    save_state_to_file(&state, "songs.json");

    // Return the newly added song as JSON
    Json(new_song)
}

async fn search_song(
    State(state): State<Arc<Mutex<AppState>>>,
    Query(params): Query<QueryParams>, // Extract search query parameters
) -> Json<Vec<Song>> {
    let state = state.lock().unwrap();

    let results: Vec<Song> = state
        .songs
        .iter()
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
    Path(id): Path<usize>
) -> Json<serde_json::Value> {

    let mut state = state.lock().unwrap();

    if let Some(song) = state.songs.iter_mut().find(|s| s.id == id) {
        song.play_count += 1;
        Json(serde_json::json!(song))
    } else {
        Json(serde_json::json!({ "error": "Song not found" }))
    }
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
            eprintln!("Error parsing state file. Starting with default state.");
            AppState::default()
        }
    }
}

// Function to save the state to a JSON file
fn save_state_to_file(state: &AppState, file_path: &str) {
    let content = match serde_json::to_string_pretty(state) {
        Ok(json) => json,
        Err(_) => {
            eprintln!("Error serializing state. Changes will not be saved.");
            return;
        }
    };

    if let Err(_) = fs::write(file_path, content) {
        eprintln!("Error writing state to file.");
    }
}
