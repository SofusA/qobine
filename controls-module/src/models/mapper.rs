use qobuz_client::{
    client::AudioQuality,
    qobuz_models::{self},
};
use time::macros::format_description;

use crate::models::{
    Album, AlbumSimple, Artist, ArtistPage, DiscoverPage, Genre, Playlist, PlaylistSimple,
    PlaylistTag, SearchResults, Track,
};

pub fn parse_featured_album(value: qobuz_models::featured::FeaturedAlbum) -> AlbumSimple {
    AlbumSimple {
        id: value.id,
        title: value.title,
        artist: parse_artist(value.artist),
        hires_available: value.hires_streamable,
        explicit: value.parental_warning,
        available: value.streamable,
        image: value.image.large,
        duration_seconds: value.duration,
        release_year: extract_year(&value.release_date_original),
    }
}

pub fn parse_search_results(
    search_results: qobuz_models::search_results::SearchAllResults,
    user_id: i64,
    max_audio_quality: &AudioQuality,
) -> SearchResults {
    SearchResults {
        query: search_results.query,
        albums: search_results
            .albums
            .items
            .into_iter()
            .map(|a| parse_album(a, max_audio_quality))
            .collect(),
        artists: search_results
            .artists
            .items
            .into_iter()
            .map(parse_artist)
            .collect(),
        playlists: search_results
            .playlists
            .items
            .into_iter()
            .map(|p| parse_playlist(p, user_id, max_audio_quality))
            .collect(),
        tracks: search_results
            .tracks
            .items
            .into_iter()
            .map(|t| parse_track(t, max_audio_quality))
            .collect(),
    }
}

pub fn parse_album_simple(
    s: qobuz_models::album_suggestion::AlbumSuggestion,
    max_audio_quality: &AudioQuality,
) -> AlbumSimple {
    let artist = s.artists.and_then(|vec| vec.into_iter().next());
    let (artist_id, artist_name) = artist.map_or((0, "Unknown".into()), |artist| {
        (artist.id as u32, artist.name.unwrap_or("Unknown".into()))
    });

    AlbumSimple {
        id: s.id,
        title: s.title,
        artist: Artist {
            id: artist_id,
            name: artist_name,
            ..Default::default()
        },
        hires_available: hifi_available(s.rights.hires_streamable, max_audio_quality),
        explicit: s.parental_warning,
        available: s.rights.streamable,
        image: s.image.large,
        duration_seconds: s.duration,
        release_year: extract_year(&s.dates.original),
    }
}

pub fn extract_year(date_str: &str) -> u32 {
    let format = format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(date_str, &format).expect("failed to parse date");
    date.year() as u32
}

pub fn parse_album(value: qobuz_models::album::Album, max_audio_quality: &AudioQuality) -> Album {
    let year = extract_year(&value.release_date_original);

    let tracks = value.tracks.map_or(Default::default(), |tracks| {
        tracks
            .items
            .into_iter()
            .map(|t| Track {
                id: t.id,
                title: t.title,
                number: t.track_number,
                explicit: t.parental_warning,
                hires_available: t.hires_streamable,
                available: t.streamable,
                status: Default::default(),
                image: Some(value.image.large.clone()),
                image_thumbnail: Some(value.image.small.clone()),
                duration_seconds: t.duration,
                artist_name: Some(value.artist.name.clone()),
                artist_id: Some(value.artist.id),
                album_title: Some(value.title.clone()),
                album_id: Some(value.id.clone()),
                playlist_track_id: None,
            })
            .collect()
    });

    Album {
        id: value.id,
        title: value.title,
        artist: parse_artist(value.artist),
        total_tracks: value.tracks_count as u32,
        release_year: year
            .to_string()
            .parse::<u32>()
            .expect("error converting year"),
        hires_available: hifi_available(value.hires_streamable, max_audio_quality),
        explicit: value.parental_warning,
        available: value.streamable,
        tracks,
        image: value.image.large,
        image_thumbnail: value.image.small,
        duration_seconds: value.duration.map_or(0, |duration| duration as u32),
        description: sanitize_html(value.description),
    }
}

fn sanitize_html(source: Option<String>) -> Option<String> {
    let source = source?;
    if source.trim() == "" {
        return None;
    }

    let mut data = String::new();
    let mut inside = false;

    for c in source.chars() {
        if c == '<' {
            inside = true;
            continue;
        }
        if c == '>' {
            inside = false;
            continue;
        }

        if !inside {
            data.push(c);
        }
    }

    Some(data.replace("&copy", "©"))
}

fn image_to_string(value: qobuz_models::artist_page::Image) -> String {
    format!(
        "https://static.qobuz.com/images/artists/covers/large/{}.{}",
        value.hash, value.format
    )
}

pub fn parse_artist_page(
    artist: qobuz_models::artist_page::ArtistPage,
    albums: Vec<AlbumSimple>,
    singles: Vec<AlbumSimple>,
    live: Vec<AlbumSimple>,
    compilations: Vec<AlbumSimple>,
    similar_artists: Vec<Artist>,
) -> ArtistPage {
    let artist_image_url = artist.images.portrait.map(image_to_string);

    ArtistPage {
        id: artist.id,
        name: artist.name.display.clone(),
        image: artist_image_url.clone(),
        albums,
        singles,
        live,
        compilations,
        similar_artists,
        top_tracks: artist
            .top_tracks
            .into_iter()
            .map(|t| {
                let album_image_url = t.album.image.large;
                let album_image_url_small = t.album.image.small;
                Track {
                    id: t.id,
                    number: t.physical_support.track_number,
                    title: t.title,
                    explicit: t.parental_warning,
                    hires_available: t.rights.hires_streamable,
                    available: t.rights.streamable,
                    status: Default::default(),
                    image: Some(album_image_url),
                    image_thumbnail: Some(album_image_url_small),
                    duration_seconds: t.duration,
                    artist_name: Some(artist.name.display.clone()),
                    artist_id: Some(artist.id),
                    album_title: Some(t.album.title),
                    album_id: Some(t.album.id),
                    playlist_track_id: None,
                }
            })
            .collect(),
        description: sanitize_html(artist.biography.map(|bio| bio.content)),
    }
}

pub fn parse_artist(value: qobuz_models::artist::Artist) -> Artist {
    Artist {
        id: value.id,
        name: value.name,
        image: value.image.map(|i| i.large),
    }
}

pub fn parse_genre(value: qobuz_models::genre::Genre) -> Genre {
    Genre {
        name: value.name,
        id: value.id,
    }
}

pub fn parse_playlist(
    playlist: qobuz_models::playlist::Playlist,
    user_id: i64,
    max_audio_quality: &AudioQuality,
) -> Playlist {
    let tracks = playlist.tracks.map_or(Default::default(), |tracks| {
        tracks
            .items
            .into_iter()
            .map(|t| parse_track(t, max_audio_quality))
            .collect()
    });

    let image = if let Some(image) = playlist.image_rectangle.first() {
        Some(image.clone())
    } else if let Some(images) = playlist.images300 {
        images.first().cloned()
    } else {
        None
    };

    Playlist {
        id: playlist.id as u32,
        is_owned: user_id == playlist.owner.id,
        title: playlist.name,
        duration_seconds: playlist.duration as u32,
        image,
        tracks,
        owner: playlist.owner,
    }
}

pub fn parse_playlist_simple(
    playlist: qobuz_models::playlist::PlaylistSimple,
    user_id: i64,
) -> PlaylistSimple {
    PlaylistSimple {
        id: playlist.id as u32,
        is_owned: user_id == playlist.owner.id,
        title: playlist.name,
        duration_seconds: playlist.duration as u32,
        tracks_count: playlist.tracks_count as u32,
        image: Some(playlist.image.rectangle),
        owner: playlist.owner,
    }
}

pub fn parse_discover(
    discover: qobuz_models::discover::Discover,
    max_audio_quality: &AudioQuality,
    user_id: i64,
) -> DiscoverPage {
    DiscoverPage {
        new_releases: discover
            .containers
            .new_releases
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        qobuzissims: discover
            .containers
            .qobuzissims
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        ideal_discography: discover
            .containers
            .ideal_discography
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        album_of_the_week: discover
            .containers
            .album_of_the_week
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        most_streamed: discover
            .containers
            .most_streamed
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        press_awards: discover
            .containers
            .press_awards
            .data
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, max_audio_quality))
            .collect(),
        playlists: discover
            .containers
            .playlists
            .data
            .items
            .into_iter()
            .map(|x| parse_playlist_simple(x, user_id))
            .collect(),
        playlists_tags: discover
            .containers
            .playlists_tags
            .data
            .items
            .into_iter()
            .map(|x| PlaylistTag {
                slug: x.slug,
                name: x.name,
            })
            .collect(),
    }
}

pub fn parse_track(value: qobuz_models::track::Track, max_audio_quality: &AudioQuality) -> Track {
    let artist = if let Some(p) = &value.performer {
        Some(Artist {
            id: p.id as u32,
            name: p.name.clone(),
            image: None,
        })
    } else {
        value.album.as_ref().map(|a| parse_artist(a.clone().artist))
    };

    let image = value.album.as_ref().map(|a| a.image.large.clone());
    let image_thumbnail = value.album.as_ref().map(|a| a.image.small.clone());

    Track {
        id: value.id,
        number: value.track_number,
        title: value.title,
        duration_seconds: value.duration,
        explicit: value.parental_warning,
        hires_available: hifi_available(value.hires_streamable, max_audio_quality),
        available: value.streamable,
        status: Default::default(),
        image,
        image_thumbnail,
        artist_name: artist.as_ref().map(move |a| a.name.clone()),
        artist_id: artist.as_ref().map(move |a| a.id),
        album_title: value.album.as_ref().map(|a| a.title.clone()),
        album_id: value.album.as_ref().map(|a| a.id.clone()),
        playlist_track_id: value.playlist_track_id,
    }
}

pub fn hifi_available(track_has_hires_available: bool, max_audio_quality: &AudioQuality) -> bool {
    if !track_has_hires_available {
        return false;
    }

    match max_audio_quality {
        AudioQuality::Mp3 => false,
        AudioQuality::CD => false,
        AudioQuality::HIFI96 => true,
        AudioQuality::HIFI192 => true,
    }
}
