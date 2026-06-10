use qobuz_player_client::client::{Client, ReleaseType};
use qobuz_player_player::database::{Credentials, Database};

async fn get_token() -> Option<Credentials> {
    let database = Database::new().await.ok()?;
    database.get_credentials().await.ok()?
}

async fn get_client() -> Option<Client> {
    let credentials = get_token().await?;

    qobuz_player_client::client::Client::new(
        &credentials.user_auth_token,
        credentials.user_id,
        qobuz_player_client::client::AudioQuality::Mp3,
        false,
    )
    .await
    .ok()
}

#[tokio::test]
async fn track_suggestion() {
    let client = get_client().await.unwrap();
    let queue = vec![20808551, 20808552, 20808553, 20808554, 20808555];

    let _suggestion = client.suggest_track(queue, None, None).await.unwrap();
}

#[tokio::test]
async fn discover_page() {
    let client = get_client().await.unwrap();

    let page = client.discover_index(None).await.unwrap();
    let genres = client.genres().await.unwrap().genres.items;

    let jazz = genres.iter().find(|x| x.name == "Jazz").unwrap().id;

    let _jazz_page = client.discover_index(Some(jazz)).await.unwrap();

    let focus_tag = page
        .containers
        .playlists_tags
        .data
        .items
        .iter()
        .find(|x| x.slug == "focus")
        .unwrap();

    let _discover_playlists = client.genre_playlists(None, None).await.unwrap();

    let _jazz_playlists = client.genre_playlists(Some(jazz), None).await.unwrap();

    let _focus_playlists = client
        .genre_playlists(None, Some(&focus_tag.slug))
        .await
        .unwrap();

    let _jazz_focus_playlists = client
        .genre_playlists(Some(jazz), Some(&focus_tag.slug))
        .await
        .unwrap();
}

#[tokio::test]
async fn user_playlists() {
    let client = get_client().await.unwrap();
    client.user_playlists().await.unwrap();
}

#[tokio::test]
async fn favorites() {
    let client = get_client().await.unwrap();
    client.favorites(3).await.unwrap();
}

#[tokio::test]
async fn playlist() {
    let client = get_client().await.unwrap();
    client.playlist(28869445).await.unwrap();
}

#[tokio::test]
async fn search() {
    let client = get_client().await.unwrap();
    client
        .search_all("a light for attracting attention", 3)
        .await
        .unwrap();
}

#[tokio::test]
async fn album() {
    let client = get_client().await.unwrap();
    client.album("mwytv5nahdbga").await.unwrap();
}

#[tokio::test]
async fn album_2() {
    let client = get_client().await.unwrap();
    client.album("dpognys4zadzb").await.unwrap();
}

#[tokio::test]
async fn track() {
    let client = get_client().await.unwrap();
    client.track(64868955).await.unwrap();
}

#[tokio::test]
async fn suggested_albums() {
    let client = get_client().await.unwrap();
    client.suggested_albums("mwytv5nahdbga").await.unwrap();
}

#[tokio::test]
async fn artist() {
    let client = get_client().await.unwrap();
    client.artist(9316383).await.unwrap();
}

#[tokio::test]
async fn similar_artist() {
    let client = get_client().await.unwrap();
    client.similar_artists(9316383, Some(3)).await.unwrap();
}

#[tokio::test]
async fn artist_releases() {
    let client = get_client().await.unwrap();
    client
        .artist_releases(9316383, ReleaseType::Albums, Some(3))
        .await
        .unwrap();
    client
        .artist_releases(9316383, ReleaseType::EPsAndSingles, Some(3))
        .await
        .unwrap();
    client
        .artist_releases(9316383, ReleaseType::Live, Some(3))
        .await
        .unwrap();
    client
        .artist_releases(9316383, ReleaseType::Compilations, Some(3))
        .await
        .unwrap();
}

// TODO: Add remaining tests
// Create playlist
// Delete playlist
// Add track to playlist
// Delete track from playlist
// Update track position in playlist

// Add favorite track, album, artist, playlist
// Remove favorite track, album, artist, playlist
