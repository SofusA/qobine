use controls_module::{
    controls::Controls,
    models::{Album, Artist, ArtistPage, Playlist, PlaylistSimple, Track},
};
use player_module::{AppResult, client::Client};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::*,
};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::{FavoriteIds, NotificationList, Output},
    ui::{block, center, centered_rect_fixed, format_seconds, render_input, tab_bar},
    widgets::{
        album_list::AlbumList,
        playlist_list::PlaylistList,
        track_list::{TrackList, TrackListEvent},
    },
};

pub struct ArtistPopupState {
    artist_name: String,
    albums: AlbumList,
    singles: AlbumList,
    live: AlbumList,
    compilations: AlbumList,
    selected_sub_tab: usize,
    top_tracks: TrackList,
    id: u32,
}

enum SelectedArtistPopupSubtabMut<'a> {
    Albums(&'a mut AlbumList),
    TopTracks(&'a mut TrackList),
}

enum SelectedArtistPopupSubtab<'a> {
    Albums(&'a AlbumList),
    TopTracks(&'a TrackList),
}

struct Tab<'a> {
    name: &'a str,
    is_empty: bool,
}

enum TabKind {
    Albums,
    TopTracks,
    Singles,
    Live,
    Compilations,
}

impl ArtistPopupState {
    pub async fn new(artist: &Artist, client: &Client) -> AppResult<Self> {
        let id = artist.id;
        let artist_page = client.artist_page(id).await?;

        let state = Self {
            artist_name: artist.name.clone(),
            albums: AlbumList::new(artist_page.albums),
            singles: AlbumList::new(artist_page.singles),
            live: AlbumList::new(artist_page.live),
            compilations: AlbumList::new(artist_page.compilations),
            selected_sub_tab: 0,
            top_tracks: TrackList::new(artist_page.top_tracks),
            id: artist.id,
        };

        Ok(state)
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;
    }

    fn cycle_subtab(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count + 1) % count;
    }

    fn visible_tab_kinds(&self) -> Vec<TabKind> {
        let mut tabs = vec![];

        if !self.albums.filter().is_empty() {
            tabs.push(TabKind::Albums);
        }
        if !self.top_tracks.filter().is_empty() {
            tabs.push(TabKind::TopTracks);
        }
        if !self.singles.filter().is_empty() {
            tabs.push(TabKind::Singles);
        }
        if !self.live.filter().is_empty() {
            tabs.push(TabKind::Live);
        }
        if !self.compilations.filter().is_empty() {
            tabs.push(TabKind::Compilations);
        }

        tabs
    }

    fn current_state_mut(&'_ mut self) -> Option<SelectedArtistPopupSubtabMut<'_>> {
        let visible_tabs = self.visible_tab_kinds();
        let tab = visible_tabs.get(self.selected_sub_tab)?;

        match tab {
            TabKind::Albums => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.albums)),
            TabKind::TopTracks => Some(SelectedArtistPopupSubtabMut::TopTracks(
                &mut self.top_tracks,
            )),
            TabKind::Singles => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.singles)),
            TabKind::Live => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.live)),
            TabKind::Compilations => {
                Some(SelectedArtistPopupSubtabMut::Albums(&mut self.compilations))
            }
        }
    }

    fn current_state(&self) -> Option<SelectedArtistPopupSubtab<'_>> {
        let visible_tabs = self.visible_tab_kinds();
        let tab = visible_tabs.get(self.selected_sub_tab)?;

        match tab {
            TabKind::Albums => Some(SelectedArtistPopupSubtab::Albums(&self.albums)),
            TabKind::TopTracks => Some(SelectedArtistPopupSubtab::TopTracks(&self.top_tracks)),
            TabKind::Singles => Some(SelectedArtistPopupSubtab::Albums(&self.singles)),
            TabKind::Live => Some(SelectedArtistPopupSubtab::Albums(&self.live)),
            TabKind::Compilations => Some(SelectedArtistPopupSubtab::Albums(&self.compilations)),
        }
    }

    fn current_row_count(&self) -> usize {
        let current_state = self.current_state();
        match current_state {
            Some(state) => match state {
                SelectedArtistPopupSubtab::Albums(album_list) => album_list.filter().len(),
                SelectedArtistPopupSubtab::TopTracks(track_list) => track_list.filter().len(),
            },
            None => 0,
        }
    }

    fn tabs(&self) -> Vec<&str> {
        vec![
            Tab {
                name: "Albums",
                is_empty: self.albums.filter().is_empty(),
            },
            Tab {
                name: "Top Tracks",
                is_empty: self.top_tracks.filter().is_empty(),
            },
            Tab {
                name: "Singles",
                is_empty: self.singles.filter().is_empty(),
            },
            Tab {
                name: "Live",
                is_empty: self.live.filter().is_empty(),
            },
            Tab {
                name: "Compilations",
                is_empty: self.compilations.filter().is_empty(),
            },
        ]
        .into_iter()
        .filter(|t| !t.is_empty)
        .map(|x| x.name)
        .collect()
    }
}

pub struct AlbumPopupState {
    title: String,
    tracks: TrackList,
    id: String,
}

impl AlbumPopupState {
    pub fn new(album: Album) -> Self {
        Self {
            title: album.title,
            tracks: TrackList::new(album.tracks),
            id: album.id,
        }
    }
}

pub struct PlaylistPopupState {
    shuffle: bool,
    tracks: TrackList,
    title: String,
    id: u32,
    is_owned: bool,
}

impl PlaylistPopupState {
    pub fn new(playlist: Playlist) -> Self {
        Self {
            tracks: TrackList::new(playlist.tracks),
            title: playlist.title,
            shuffle: false,
            id: playlist.id,
            is_owned: playlist.is_owned,
        }
    }
}

pub struct DeletePlaylistPopupState {
    title: String,
    id: u32,
    confirm: bool,
}

impl DeletePlaylistPopupState {
    pub fn new(playlist: PlaylistSimple) -> Self {
        Self {
            title: playlist.title,
            id: playlist.id,
            confirm: false,
        }
    }
}

pub struct TrackPopupState {
    playlists: PlaylistList,
    track: Track,
}

impl TrackPopupState {
    pub fn new(track: Track, owned_playlists: Vec<PlaylistSimple>) -> Self {
        Self {
            playlists: PlaylistList::new(owned_playlists),
            track,
        }
    }

    fn select_next(&mut self) {
        self.playlists.select_next();
    }

    fn select_previous(&mut self) {
        self.playlists.select_previous();
    }
}

pub struct NewPlaylistPopupState {
    name: Input,
}

impl NewPlaylistPopupState {
    pub fn new() -> Self {
        Self {
            name: Default::default(),
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Popup {
    Artist(ArtistPopupState),
    Album(AlbumPopupState),
    Playlist(PlaylistPopupState),
    Track(TrackPopupState),
    NewPlaylist(NewPlaylistPopupState),
    DeletePlaylist(DeletePlaylistPopupState),
    AlbumInfo(Album, bool, Option<(StatefulProtocol, f32)>),
    ArtistInfo(ArtistPage, Option<(StatefulProtocol, f32)>),
    PlaylistInfo(Playlist, Option<(StatefulProtocol, f32)>),
    TrackInfo(Track, Option<(StatefulProtocol, f32)>),
}

impl Popup {
    pub fn render(&mut self, frame: &mut Frame, favorite_ids: &FavoriteIds) {
        match self {
            Popup::Album(state) => {
                let area = center(
                    frame.area(),
                    Constraint::Percentage(50),
                    Constraint::Length(state.tracks.filter().len() as u16 + 2),
                );

                let block = block(Some(&state.title));

                frame.render_widget(Clear, area);
                frame.render_widget(&block, area);
                state.tracks.render(
                    block.inner(area),
                    frame.buffer_mut(),
                    false,
                    true,
                    &favorite_ids.tracks,
                );
            }
            Popup::Artist(artist) => {
                let visible_rows = (artist.current_row_count() + 1).min(15) as u16;

                let tabs_height: u16 = 2;
                let border_height: u16 = 2;
                let min_height: u16 = 4;

                let popup_height = (visible_rows + border_height + tabs_height)
                    .clamp(min_height, frame.area().height.saturating_sub(2));

                let popup_width = (frame.area().width * 75 / 100).max(30);

                let area = centered_rect_fixed(popup_width, popup_height, frame.area());

                let outer_block = block(Some(&artist.artist_name));

                let tabs = tab_bar(artist.tabs(), artist.selected_sub_tab);

                frame.render_widget(Clear, area);
                frame.render_widget(&outer_block, area);

                let inner = outer_block.inner(area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(tabs_height), Constraint::Min(1)])
                    .split(inner);

                frame.render_widget(tabs, chunks[0]);

                if let Some(state) = artist.current_state_mut() {
                    match state {
                        SelectedArtistPopupSubtabMut::Albums(album_list) => album_list.render(
                            chunks[1],
                            frame.buffer_mut(),
                            true,
                            &favorite_ids.albums,
                        ),
                        SelectedArtistPopupSubtabMut::TopTracks(track_list) => track_list.render(
                            chunks[1],
                            frame.buffer_mut(),
                            true,
                            true,
                            &favorite_ids.tracks,
                        ),
                    }
                }
            }
            Popup::Playlist(playlist_state) => {
                let visible_rows = playlist_state.tracks.filter().len().min(15) as u16;

                let inner_content_height = visible_rows + 3;
                let block_border_height = 2;

                let popup_height = (inner_content_height + block_border_height)
                    .clamp(4, frame.area().height.saturating_sub(2));

                let popup_width = (frame.area().width * 75 / 100).max(30);

                let area = centered_rect_fixed(popup_width, popup_height, frame.area());

                let buttons = tab_bar(
                    ["Play", "Shuffle"].into(),
                    if playlist_state.shuffle { 1 } else { 0 },
                );

                let block = block(Some(&playlist_state.title));

                frame.render_widget(Clear, area);

                let inner = block.inner(area);
                frame.render_widget(block, area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ])
                    .split(inner);

                playlist_state.tracks.render(
                    chunks[0],
                    frame.buffer_mut(),
                    true,
                    true,
                    &favorite_ids.tracks,
                );
                frame.render_widget(buttons, chunks[2]);
            }
            Popup::Track(track_state) => {
                let area = center(
                    frame.area(),
                    Constraint::Percentage(75),
                    Constraint::Percentage(50),
                );

                let block_title = format!("Add {} to playlist", track_state.track.title);
                let block = block(Some(&block_title));

                frame.render_widget(Clear, area);
                frame.render_widget(&block, area);
                track_state
                    .playlists
                    .render(block.inner(area), frame.buffer_mut(), true);
            }
            Popup::NewPlaylist(state) => {
                let area = center(
                    frame.area(),
                    Constraint::Percentage(75),
                    Constraint::Length(3),
                );

                frame.render_widget(Clear, area);
                render_input(&state.name, false, area, frame, "Create playlist");
            }
            Popup::DeletePlaylist(state) => {
                let block_title = format!("Delete {}?", state.title);
                let area = center(
                    frame.area(),
                    Constraint::Length(block_title.chars().count() as u16 + 6),
                    Constraint::Length(3),
                );

                let tabs = tab_bar(
                    ["Delete", "Cancel"].into(),
                    if state.confirm { 0 } else { 1 },
                )
                .block(block(Some(&block_title)));

                frame.render_widget(Clear, area);
                frame.render_widget(tabs, area);
            }
            Popup::AlbumInfo(album, currently_playing, image) => {
                render_album_info(frame, album, *currently_playing, image);
            }
            Popup::ArtistInfo(artist, image) => {
                render_artist_info(frame, artist, image);
            }
            Popup::PlaylistInfo(playlist, image) => {
                render_playlist_info(frame, playlist, image);
            }
            Popup::TrackInfo(track, image) => {
                render_track_info(frame, track, image);
            }
        };
    }

    pub async fn handle_event(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self {
                Popup::AlbumInfo(_, _, _) => Ok(Output::Consumed),
                Popup::ArtistInfo(_, _) => Ok(Output::Consumed),
                Popup::PlaylistInfo(_, _) => Ok(Output::Consumed),
                Popup::TrackInfo(_, _) => Ok(Output::Consumed),
                Popup::Album(album_state) => {
                    album_state
                        .tracks
                        .handle_events(
                            key_event.code,
                            client,
                            controls,
                            notifications,
                            TrackListEvent::Album(album_state.id.clone()),
                        )
                        .await
                }
                Popup::Artist(artist_popup_state) => match key_event.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        artist_popup_state.cycle_subtab_backwards();
                        Ok(Output::Consumed)
                    }

                    KeyCode::Right | KeyCode::Char('l') => {
                        artist_popup_state.cycle_subtab();
                        Ok(Output::Consumed)
                    }
                    _ => {
                        let artist_id = artist_popup_state.id;
                        let current_state = artist_popup_state.current_state_mut();
                        match current_state {
                            Some(state) => match state {
                                SelectedArtistPopupSubtabMut::Albums(album_list) => {
                                    album_list
                                        .handle_events(
                                            key_event.code,
                                            client,
                                            controls,
                                            notifications,
                                        )
                                        .await
                                }
                                SelectedArtistPopupSubtabMut::TopTracks(track_list) => {
                                    track_list
                                        .handle_events(
                                            key_event.code,
                                            client,
                                            controls,
                                            notifications,
                                            TrackListEvent::Artist(artist_id),
                                        )
                                        .await
                                }
                            },
                            None => Ok(Output::Consumed),
                        }
                    }
                },
                Popup::Playlist(playlist_popup_state) => match key_event.code {
                    KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
                        playlist_popup_state.shuffle = !playlist_popup_state.shuffle;
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('D') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                client
                                    .playlist_delete_track(
                                        playlist_popup_state.id,
                                        &[playlist_track_id],
                                    )
                                    .await?;
                                playlist_popup_state.tracks.remove_at_index(index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('u') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                let new_index = index - 1;
                                client
                                    .update_playlist_track_position(
                                        new_index,
                                        playlist_popup_state.id,
                                        playlist_track_id,
                                    )
                                    .await?;

                                playlist_popup_state
                                    .tracks
                                    .move_index_to_new_index(index, new_index);

                                playlist_popup_state.tracks.select_index(new_index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('d') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                let new_index = index + 1;
                                client
                                    .update_playlist_track_position(
                                        new_index,
                                        playlist_popup_state.id,
                                        playlist_track_id,
                                    )
                                    .await?;

                                playlist_popup_state
                                    .tracks
                                    .move_index_to_new_index(index, new_index);

                                playlist_popup_state.tracks.select_index(new_index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    _ => {
                        playlist_popup_state
                            .tracks
                            .handle_events(
                                key_event.code,
                                client,
                                controls,
                                notifications,
                                TrackListEvent::Playlist(
                                    playlist_popup_state.id,
                                    playlist_popup_state.shuffle,
                                ),
                            )
                            .await
                    }
                },
                Popup::Track(track_popup_state) => match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        track_popup_state.select_previous();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        track_popup_state.select_next();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Enter => {
                        let index = track_popup_state.playlists.selected();
                        let id = index
                            .and_then(|index| track_popup_state.playlists.get(index))
                            .map(|p| p.id);

                        if let Some(id) = id {
                            return Ok(Output::AddTrackToPlaylistAndPopPopup((
                                track_popup_state.track.id,
                                id,
                            )));
                        }

                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                Popup::NewPlaylist(state) => match key_event.code {
                    KeyCode::Enter => {
                        let input = state.name.value();
                        client
                            .create_playlist(input.to_string(), false, Default::default(), None)
                            .await?;
                        Ok(Output::PopPopupUpdateFavorites)
                    }
                    _ => {
                        state.name.handle_event(&event);
                        Ok(Output::Consumed)
                    }
                },
                Popup::DeletePlaylist(state) => match key_event.code {
                    KeyCode::Enter => {
                        if state.confirm {
                            client.delete_playlist(state.id).await?;
                            return Ok(Output::PopPopupUpdateFavorites);
                        }

                        Ok(Output::PopPopupUpdateFavorites)
                    }
                    KeyCode::Left | KeyCode::Right => {
                        state.confirm = !state.confirm;
                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::Consumed),
                },
            },
            _ => Ok(Output::Consumed),
        }
    }
}

fn render_album_info(
    frame: &mut Frame,
    album: &Album,
    currently_playing: bool,
    image: &mut Option<(StatefulProtocol, f32)>,
) {
    let mut info_lines: Vec<Line> = Vec::new();

    info_lines.push(Line::from(album.title.clone()).style(Style::new().bold()));
    info_lines.push(Line::from(album.artist.name.clone()));
    info_lines.push(Line::from(""));

    if album.release_year > 0 {
        info_lines.push(Line::from(format!("Year:     {}", album.release_year)));
    }

    info_lines.push(Line::from(format!("Tracks:   {}", album.total_tracks)));
    info_lines.push(Line::from(format!(
        "Duration: {}",
        format_seconds(album.duration_seconds)
    )));

    if album.hires_available {
        info_lines.push(Line::from("Quality:  Hi-Res"));
    }

    if album.explicit {
        info_lines.push(Line::from("Explicit: Yes"));
    }

    let info_height = info_lines.len() as u16;

    let box_width = frame.area().width - 20;
    let inner_width = box_width.saturating_sub(2);

    let desc_height = if let Some(description) = &album.description {
        let char_count = description.len() as u16;
        let lines_needed = char_count.div_ceil(inner_width.max(1));
        1 + lines_needed // 1 for blank separator line
    } else {
        0
    };

    let total_height = info_height + desc_height + 2;

    let width = Constraint::Length(box_width);
    let height = Constraint::Length(total_height);
    let area = center(frame.area(), width, height);
    let title = match currently_playing {
        true => "Currently playing album info",
        false => "Album info",
    };
    let outer_block = block(Some(title));
    let inner = outer_block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(outer_block, area);

    let vertical =
        Layout::vertical([Constraint::Length(info_height), Constraint::Min(0)]).split(inner);

    let top_area = vertical[0];
    let desc_area = vertical[1];

    let image_width = if let Some((_, ratio)) = image {
        (*ratio * (top_area.height * 2) as f32) as u16
    } else {
        0
    };

    let horizontal =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(image_width)]).split(top_area);

    let info_paragraph = Paragraph::new(Text::from(info_lines));
    frame.render_widget(info_paragraph, horizontal[0]);

    if let Some((protocol, _)) = image {
        let stateful_image = StatefulImage::default();
        frame.render_stateful_widget(stateful_image, horizontal[1], protocol);
    }

    if let Some(description) = &album.description {
        let desc_lines = vec![Line::from(""), Line::from(description.clone())];
        let desc_paragraph = Paragraph::new(Text::from(desc_lines)).wrap(Wrap { trim: false });
        frame.render_widget(desc_paragraph, desc_area);
    }
}

fn render_artist_info(
    frame: &mut Frame,
    artist: &ArtistPage,
    image: &mut Option<(StatefulProtocol, f32)>,
) {
    let mut info_lines: Vec<Line> = Vec::new();

    info_lines.push(Line::from(artist.name.clone()).style(Style::new().bold()));
    info_lines.push(Line::from(""));
    info_lines.push(Line::from(format!("Albums:  {}", artist.albums.len())));

    let info_height = info_lines.len() as u16;

    let box_width = frame.area().width - 20;
    let inner_width = box_width.saturating_sub(2);

    let desc_height = if let Some(description) = &artist.description {
        let char_count = description.len() as u16;
        let lines_needed = char_count.div_ceil(inner_width.max(1));
        1 + lines_needed
    } else {
        0
    };

    let total_height = info_height + desc_height + 2;

    let width = Constraint::Length(box_width);
    let height = Constraint::Length(total_height);
    let area = center(frame.area(), width, height);

    let title = "Artist info";

    let outer_block = block(Some(title));
    let inner = outer_block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(outer_block, area);

    let extra_image_lines = 5;

    let vertical = Layout::vertical([
        Constraint::Length(info_height + extra_image_lines),
        Constraint::Min(0),
    ])
    .split(inner);

    let top_area = vertical[0];
    let desc_area = vertical[1];

    let image_width = if let Some((_, ratio)) = image {
        (*ratio * (top_area.height * 2) as f32) as u16
    } else {
        0
    };

    let horizontal =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(image_width)]).split(top_area);

    let info_paragraph = Paragraph::new(Text::from(info_lines));
    frame.render_widget(info_paragraph, horizontal[0]);

    if let Some((protocol, _)) = image {
        let stateful_image = StatefulImage::default();
        frame.render_stateful_widget(stateful_image, horizontal[1], protocol);
    }

    if let Some(description) = &artist.description {
        let desc_lines = vec![Line::from(""), Line::from(description.clone())];
        let desc_paragraph = Paragraph::new(Text::from(desc_lines)).wrap(Wrap { trim: false });
        frame.render_widget(desc_paragraph, desc_area);
    }
}

fn render_track_info(
    frame: &mut Frame,
    track: &Track,
    image: &mut Option<(StatefulProtocol, f32)>,
) {
    let mut info_lines: Vec<Line> = Vec::new();

    info_lines.push(Line::from(track.title.clone()).style(Style::new().bold()));

    let artist_name = track
        .artist_name
        .clone()
        .unwrap_or_else(|| "Unknown artist".to_string());

    let album_title = track
        .album_title
        .clone()
        .unwrap_or_else(|| "Unknown album".to_string());

    info_lines.push(Line::from(artist_name));
    info_lines.push(Line::from(album_title));
    info_lines.push(Line::from(""));

    info_lines.push(Line::from(format!(
        "Duration seconds: {}",
        track.duration_seconds
    )));

    info_lines.push(Line::from(format!(
        "Duration:         {}",
        format_seconds(track.duration_seconds)
    )));

    info_lines.push(Line::from(format!(
        "Explicit:         {}",
        if track.explicit { "Yes" } else { "No" }
    )));

    info_lines.push(Line::from(format!(
        "Hi-Res:           {}",
        if track.hires_available { "Yes" } else { "No" }
    )));

    let info_height = info_lines.len() as u16;

    let box_width = frame.area().width - 20;
    let total_height = info_height + 2;

    let width = Constraint::Length(box_width);
    let height = Constraint::Length(total_height);
    let area = center(frame.area(), width, height);

    let title = "Track info";

    let outer_block = block(Some(title));
    let inner = outer_block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(outer_block, area);

    let image_width = if let Some((_, ratio)) = image {
        (*ratio * (inner.height * 2) as f32) as u16
    } else {
        0
    };

    let horizontal =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(image_width)]).split(inner);

    let info_paragraph = Paragraph::new(Text::from(info_lines));
    frame.render_widget(info_paragraph, horizontal[0]);

    if let Some((protocol, _)) = image {
        let stateful_image = StatefulImage::default();
        frame.render_stateful_widget(stateful_image, horizontal[1], protocol);
    }
}

fn render_playlist_info(
    frame: &mut Frame,
    playlist: &Playlist,
    image: &mut Option<(StatefulProtocol, f32)>,
) {
    let mut info_lines: Vec<Line> = Vec::new();

    info_lines.push(Line::from(playlist.title.clone()).style(Style::new().bold()));
    info_lines.push(Line::from(playlist.owner.name.clone()));
    info_lines.push(Line::from(""));

    info_lines.push(Line::from(format!("Tracks:   {}", playlist.tracks.len())));

    info_lines.push(Line::from(format!(
        "Duration: {}",
        format_seconds(playlist.duration_seconds)
    )));

    let info_height = info_lines.len() as u16;

    let box_width = frame.area().width - 20;
    let total_height = info_height + 2;

    let width = Constraint::Length(box_width);
    let height = Constraint::Length(total_height);
    let area = center(frame.area(), width, height);

    let title = "Playlist info";

    let outer_block = block(Some(title));
    let inner = outer_block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(outer_block, area);

    let image_width = if let Some((_, ratio)) = image {
        (*ratio * (inner.height * 2) as f32) as u16
    } else {
        0
    };

    let horizontal =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(image_width)]).split(inner);

    let info_paragraph = Paragraph::new(Text::from(info_lines));
    frame.render_widget(info_paragraph, horizontal[0]);

    if let Some((protocol, _)) = image {
        let stateful_image = StatefulImage::default();
        frame.render_stateful_widget(stateful_image, horizontal[1], protocol);
    }
}
