use axum::{
    extract::{Json, Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, atomic::{AtomicU32, Ordering}}
};

use sqlx::{migrate::MigrateDatabase, Sqlite, SqlitePool, sqlite::SqlitePoolOptions, QueryBuilder};

const DB_URL: &str = "sqlite://songs.db";

// Define the Song struct with serialization and deserialization
#[derive(sqlx::FromRow, Debug, Serialize, Deserialize, Clone)]
struct Song {
    id: i32,
    title: String,
    artist: String,
    genre: String,
    play_count: i32,
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
struct AppState {
    db: SqlitePool,
    visit_count: Arc<AtomicU32>,
}

#[tokio::main]
async fn main() {

    if !Sqlite::database_exists(DB_URL).await.unwrap_or(false) {
            println!("Creating database {}", DB_URL);
            match Sqlite::create_database(DB_URL).await {
                Ok(_) => println!("Create db success"),
                Err(error) => panic!("error: {}", error),
            }
        } else {
            println!("Database already exists");
        }

    let visit_count = Arc::new(AtomicU32::new(0));

    let db = SqlitePoolOptions::new()
        .min_connections(15)
        .max_connections(20)
        .connect(DB_URL)
        .await
        .unwrap();


    sqlx::query("
        CREATE TABLE IF NOT EXISTS songs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title_lowercase  VARCHAR(250) NOT NULL,
            genre_lowercase  VARCHAR(250) NOT NULL,
            artist_lowercase VARCHAR(250) NOT NULL,
            title VARCHAR(250) NOT NULL,
            genre VARCHAR(250) NOT NULL,
            artist VARCHAR(250) NOT NULL,
            play_count INTEGER DEFAULT 0
        );")
        .execute(&db)
        .await
        .unwrap();

    let state = AppState { db, visit_count };

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
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handle_count(State(state): State<AppState>) -> String {

    let mut count = state.visit_count.fetch_add(1, Ordering::Relaxed) + 1;
    count += 1;
    format!("Visit count: {}", count)
}

// Handler to add a new song
async fn add_new_song(
    State(state): State<AppState>,
    Json(payload): Json<NewSong>,
) -> Json<Song> {
    // Insert the new song into the database

    let result = sqlx::query("
        INSERT INTO songs (title_lowercase, artist_lowercase, genre_lowercase, title, artist, genre, play_count)
        VALUES (?, ?, ?, ?, ?, ?, 0)
    ")
    .bind(&payload.title.to_lowercase())
    .bind(&payload.artist.to_lowercase())
    .bind(&payload.genre.to_lowercase())
    .bind(&payload.title)
    .bind(&payload.artist)
    .bind(&payload.genre)
    .execute(&state.db)
    .await
    .unwrap();

    let song_id = result.last_insert_rowid() as i32;

    Json(
        Song {
            id: song_id,
            title: payload.title,
            artist: payload.artist,
            genre: payload.genre,
            play_count: 0,
        }
    )
}


// Handler to search songs based on query parameters
async fn search_song(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<Song>> {
    let db = &state.db;

    // Start building the query
    let mut query_builder = QueryBuilder::<Sqlite>::new("SELECT * FROM songs WHERE 1=1");

    // Dynamically add conditions based on query params
    if let Some(title) = &params.title {
        query_builder.push(" AND title_lowercase LIKE ").push_bind(format!("%{}%", title.to_lowercase()));
    }
    if let Some(artist) = &params.artist {
        query_builder.push(" AND artist_lowercase LIKE ").push_bind(format!("%{}%", artist.to_lowercase()));
    }
    if let Some(genre) = &params.genre {
        query_builder.push(" AND genre_lowercase LIKE ").push_bind(format!("%{}%", genre.to_lowercase()));
    }

    // Execute the query and fetch results
    let songs: Vec<Song> = query_builder
        .build_query_as::<Song>() // Map rows to the Song struct
        .fetch_all(db)
        .await
        .unwrap_or_else(|_| Vec::new()); // Handle errors gracefully by returning an empty list

    // Return the results as a JSON response
    Json(songs)
}

// // Handler to play a song by ID
async fn play_song(
    State(state): State<AppState>,
    Path(id): Path<usize>,
) -> Json<serde_json::Value> {
    let db = &state.db;

    // Increment the play_count for the song with the given ID
    let rows_affected = sqlx::query(
        "
        UPDATE songs
        SET play_count = play_count + 1
        WHERE id = ?
        "
    )
    .bind(id as i32) // Binding the ID
    .execute(db) // Execute the query
    .await
    .unwrap()
    .rows_affected();

    // Check if the song exists
    if rows_affected == 0 {
        return Json(serde_json::json!({"error": "Song not found"}));
    }

    // Fetch the updated song details
    let song: (i32, String, String, String, i32) = sqlx::query_as("
        SELECT id, title, artist, genre, play_count
        FROM songs
        WHERE id = ?
    "
    )
    .bind(id as i32)
    .fetch_one(db) // Fetch the single row
    .await
    .unwrap();

    // Return the updated song as a JSON response
    Json(serde_json::json!(Song {
        id: song.0,
        title: song.1,
        genre: song.2,
        artist: song.3,
        play_count: song.4
    }))
}
