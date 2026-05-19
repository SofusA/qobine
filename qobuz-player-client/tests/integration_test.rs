use qobuz_player_client::client::{
    Client, FeaturedAlbumType, FeaturedGenreAlbumType, FeaturedPlaylistType, ReleaseType,
};
use qobuz_player_controls::database::{Credentials, Database};

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
async fn featured_albums() {
    let client = get_client().await.unwrap();

    client
        .featured_albums(FeaturedAlbumType::PressAwards)
        .await
        .unwrap();

    client
        .featured_albums(FeaturedAlbumType::MostStreamed)
        .await
        .unwrap();

    client
        .featured_albums(FeaturedAlbumType::NewReleases)
        .await
        .unwrap();

    client
        .featured_albums(FeaturedAlbumType::Qobuzissims)
        .await
        .unwrap();

    client
        .featured_albums(FeaturedAlbumType::IdealDiscography)
        .await
        .unwrap();
}

#[tokio::test]
async fn featured_playlists() {
    let client = get_client().await.unwrap();

    client
        .featured_playlists(FeaturedPlaylistType::EditorsPick)
        .await
        .unwrap();
}

#[tokio::test]
async fn genres() {
    let client = get_client().await.unwrap();

    let genres = client.genres().await.unwrap().genres.items;

    for genre in genres {
        client.genre_playlists(genre.id).await.unwrap();
        client
            .genre_albums(genre.id, FeaturedGenreAlbumType::PressAwards)
            .await
            .unwrap();
        client
            .genre_albums(genre.id, FeaturedGenreAlbumType::MostStreamed)
            .await
            .unwrap();
        client
            .genre_albums(genre.id, FeaturedGenreAlbumType::NewReleases)
            .await
            .unwrap();
        client
            .genre_albums(genre.id, FeaturedGenreAlbumType::Qobuzissims)
            .await
            .unwrap();
        client
            .genre_albums(genre.id, FeaturedGenreAlbumType::BestSellers)
            .await
            .unwrap();
    }
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
