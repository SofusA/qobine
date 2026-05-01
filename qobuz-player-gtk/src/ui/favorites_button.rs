use gtk4::glib;
use gtk4::prelude::*;
use std::{cell::Cell, rc::Rc, sync::Arc};
use tokio::sync::mpsc;

use qobuz_player_controls::client::Client;

use crate::UiEvent;

#[derive(Clone)]
pub enum FavoriteButtonType {
    Album(String),
    Artist(u32),
    Playlist(u32),
}

pub fn new_favorite_button(
    client: Arc<Client>,
    button_type: FavoriteButtonType,
    tx: mpsc::UnboundedSender<UiEvent>,
) -> gtk4::Button {
    let is_favorite = Rc::new(Cell::new(false));
    let button_type = Rc::new(button_type);

    let favorites_button = gtk4::Button::builder()
        .label("Favorite")
        .icon_name("non-starred-symbolic")
        .css_classes(vec!["pill"])
        .build();

    glib::MainContext::default().spawn_local(glib::clone!(
        #[weak]
        favorites_button,
        #[strong]
        client,
        #[strong]
        button_type,
        #[strong]
        is_favorite,
        async move {
            if let Ok(favorites) = client.favorites().await {
                let fav = match &*button_type {
                    FavoriteButtonType::Album(album_id) => {
                        favorites.albums.iter().any(|x| x.id == *album_id)
                    }
                    FavoriteButtonType::Artist(artist_id) => {
                        favorites.artists.iter().any(|x| x.id == *artist_id)
                    }
                    FavoriteButtonType::Playlist(playlist_id) => {
                        favorites.playlists.iter().any(|x| x.id == *playlist_id)
                    }
                };

                is_favorite.set(fav);
                favorites_button.set_icon_name(if fav {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                });
            }
        }
    ));

    favorites_button.connect_clicked(glib::clone!(
        #[weak]
        favorites_button,
        #[strong]
        client,
        #[strong]
        button_type,
        #[strong]
        tx,
        #[strong]
        is_favorite,
        move |_| {
            let client = client.clone();
            let button_type = button_type.clone();

            glib::MainContext::default().spawn_local(glib::clone!(
                #[weak]
                favorites_button,
                #[strong]
                tx,
                #[strong]
                is_favorite,
                async move {
                    let next = !is_favorite.get();

                    let res = match &*button_type {
                        FavoriteButtonType::Album(album_id) => {
                            if next {
                                client.add_favorite_album(album_id).await
                            } else {
                                client.remove_favorite_album(album_id).await
                            }
                        }
                        FavoriteButtonType::Artist(artist_id) => {
                            if next {
                                client.add_favorite_artist(*artist_id).await
                            } else {
                                client.remove_favorite_artist(*artist_id).await
                            }
                        }
                        FavoriteButtonType::Playlist(playlist_id) => {
                            if next {
                                client.add_favorite_playlist(*playlist_id).await
                            } else {
                                client.remove_favorite_playlist(*playlist_id).await
                            }
                        }
                    };

                    if res.is_ok() {
                        is_favorite.set(next);
                        favorites_button.set_icon_name(if next {
                            "starred-symbolic"
                        } else {
                            "non-starred-symbolic"
                        });
                        let _ = tx.send(UiEvent::FavoritesChanged);
                    }
                }
            ));
        }
    ));

    favorites_button
}
