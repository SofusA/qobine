use qobuz_client::{
    client::AudioQuality,
    qobuz_models::{self},
};
use time::macros::format_description;

use crate::models::{
    Album, AlbumSimple, Artist, ArtistPage, DiscoverPage, Genre, Playlist, PlaylistSimple,
    PlaylistTag, SearchResults, Track, TrackStatus,
};

#[must_use]
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

#[must_use]
pub fn parse_album_simple(
    s: qobuz_models::album_suggestion::AlbumSuggestion,
    max_audio_quality: &AudioQuality,
) -> AlbumSimple {
    let artist = s.artists.and_then(|vec| vec.into_iter().next());
    let (artist_id, artist_name) = artist.map_or_else(
        || (0, "Unknown".into()),
        |artist| (artist.id, artist.name.unwrap_or_else(|| "Unknown".into())),
    );

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

// TODO: Change return type to Result
#[must_use]
pub fn extract_year(date_str: &str) -> u32 {
    let format = format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(date_str, &format);

    date.map_or(0, |date| u32::try_from(date.year()).unwrap_or_default())
}

#[must_use]
pub fn parse_album(value: qobuz_models::album::Album, max_audio_quality: &AudioQuality) -> Album {
    let year = extract_year(&value.release_date_original);

    let tracks = value.tracks.map_or_else(Vec::default, |tracks| {
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
                status: TrackStatus::default(),
                image: Some(value.image.large.clone()),
                image_thumbnail: Some(value.image.small.clone()),
                duration_seconds: t.duration,
                artist_name: Some(value.artist.name.clone()),
                artist_id: Some(value.artist.id),
                album_title: Some(value.title.clone()),
                album_id: Some(value.id.clone()),
                playlist_track_id: None,
                bit_depth: t.maximum_bit_depth,
                sampling_rate: t.maximum_sampling_rate,
                release_date: Some(value.release_date_original.clone()),
                performers: t.performers,
                copyright: t.copyright,
            })
            .collect()
    });

    Album {
        id: value.id,
        title: value.title,
        artist: parse_artist(value.artist),
        total_tracks: value.tracks_count,
        release_year: year,
        hires_available: hifi_available(value.hires_streamable, max_audio_quality),
        explicit: value.parental_warning,
        available: value.streamable,
        tracks,
        image: value.image.large,
        image_thumbnail: value.image.small,
        duration_seconds: value.duration.unwrap_or_default(),
        description: sanitize_html(value.description),
        bit_depth: value.maximum_bit_depth,
        sampling_rate: value.maximum_sampling_rate,
        awards: value.awards.into_iter().map(|x| x.name).collect(),
        label: value.label.map(|x| x.name),
    }
}

fn sanitize_html(source: Option<String>) -> Option<String> {
    let source = source?;
    if source.trim() == "" {
        return None;
    }

    let mut data = String::new();
    let mut tag = String::new();
    let mut inside = false;

    for c in source.chars() {
        if c == '<' {
            inside = true;
            tag.clear();
            continue;
        }
        if c == '>' {
            inside = false;
            let name = tag
                .split(|c: char| c.is_whitespace() || c == '/')
                .find(|s| !s.is_empty())
                .unwrap_or("");
            if name.eq_ignore_ascii_case("br") || name.eq_ignore_ascii_case("p") {
                let trailing_newlines = data.chars().rev().take_while(|&c| c == '\n').count();
                if trailing_newlines < 2 {
                    data.push('\n');
                }
            }
            continue;
        }

        if inside {
            tag.push(c);
        } else {
            data.push(c);
        }
    }

    let data = html_escape::decode_html_entities(data.trim());
    if data.is_empty() {
        return None;
    }

    Some(data.into_owned())
}

fn image_to_string(value: &qobuz_models::artist_page::Image) -> String {
    format!(
        "https://static.qobuz.com/images/artists/covers/large/{}.{}",
        value.hash, value.format
    )
}

#[must_use]
pub fn parse_artist_page(
    artist: qobuz_models::artist_page::ArtistPage,
    albums: Vec<AlbumSimple>,
    singles: Vec<AlbumSimple>,
    live: Vec<AlbumSimple>,
    compilations: Vec<AlbumSimple>,
    similar_artists: Vec<Artist>,
) -> ArtistPage {
    let artist_image_url = artist.images.portrait.map(|e| image_to_string(&e));

    ArtistPage {
        id: artist.id,
        name: artist.name.display.clone(),
        image: artist_image_url,
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
                    status: TrackStatus::default(),
                    image: Some(album_image_url),
                    image_thumbnail: Some(album_image_url_small),
                    duration_seconds: t.duration,
                    artist_name: Some(artist.name.display.clone()),
                    artist_id: Some(artist.id),
                    album_title: Some(t.album.title),
                    album_id: Some(t.album.id),
                    playlist_track_id: None,
                    bit_depth: None,
                    sampling_rate: None,
                    release_date: None,
                    performers: None,
                    copyright: None,
                }
            })
            .collect(),
        description: sanitize_html(artist.biography.map(|bio| bio.content)),
    }
}

#[must_use]
pub fn parse_artist(value: qobuz_models::artist::Artist) -> Artist {
    Artist {
        id: value.id,
        name: value.name,
        image: value.image.map(|i| i.large),
    }
}

#[must_use]
pub fn parse_genre(value: qobuz_models::genre::Genre) -> Genre {
    Genre {
        name: value.name,
        id: value.id,
    }
}

#[must_use]
pub fn parse_playlist(
    playlist: qobuz_models::playlist::Playlist,
    user_id: i64,
    max_audio_quality: &AudioQuality,
) -> Playlist {
    let tracks = playlist.tracks.map_or_else(Vec::default, |tracks| {
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
        id: playlist.id,
        is_owned: user_id == playlist.owner.id,
        title: playlist.name,
        duration_seconds: playlist.duration,
        image,
        tracks,
        owner: playlist.owner,
    }
}

#[must_use]
pub fn parse_playlist_simple(
    playlist: qobuz_models::playlist::PlaylistSimple,
    user_id: i64,
) -> PlaylistSimple {
    PlaylistSimple {
        id: playlist.id,
        is_owned: user_id == playlist.owner.id,
        title: playlist.name,
        duration_seconds: playlist.duration,
        tracks_count: playlist.tracks_count,
        image: playlist.image.rectangle,
        owner: playlist.owner,
    }
}

#[must_use]
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

#[must_use]
pub fn parse_track(value: qobuz_models::track::Track, max_audio_quality: &AudioQuality) -> Track {
    let artist = if let Some(p) = &value.performer {
        Some(Artist {
            id: p.id,
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
        status: TrackStatus::default(),
        image,
        image_thumbnail,
        artist_name: artist.as_ref().map(move |a| a.name.clone()),
        artist_id: artist.as_ref().map(move |a| a.id),
        album_title: value.album.as_ref().map(|a| a.title.clone()),
        album_id: value.album.as_ref().map(|a| a.id.clone()),
        playlist_track_id: value.playlist_track_id,
        bit_depth: value.maximum_bit_depth,
        sampling_rate: value.maximum_sampling_rate,
        release_date: value.release_date_original.or_else(|| {
            value
                .album
                .as_ref()
                .map(|a| a.release_date_original.clone())
        }),
        performers: value.performers,
        copyright: value.copyright,
    }
}

#[must_use]
pub const fn hifi_available(
    track_has_hires_available: bool,
    max_audio_quality: &AudioQuality,
) -> bool {
    if !track_has_hires_available {
        return false;
    }

    match max_audio_quality {
        AudioQuality::Mp3 | AudioQuality::CD => false,
        AudioQuality::HIFI96 | AudioQuality::HIFI192 => true,
    }
}
