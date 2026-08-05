use std::collections::HashMap;

use image::load_from_memory;
use num_traits::ToPrimitive;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use tokio::sync::mpsc;

#[allow(clippy::large_enum_variant)]
enum CachedImage {
    Loading,
    Ready(AppImage),
    Failed,
}

pub struct ImageLoaded {
    url: String,
    image: Option<AppImage>,
}

type ImageCacheMap = HashMap<String, CachedImage>;

pub struct AppImage {
    pub protocol: StatefulProtocol,
    pub ratio: f32,
}

pub struct ImageManager {
    cache: ImageCacheMap,
    picker: Picker,
    http_client: reqwest::Client,
    tx: mpsc::UnboundedSender<ImageLoaded>,
}

impl ImageManager {
    pub fn new(picker: Picker, sender: mpsc::UnboundedSender<ImageLoaded>) -> Self {
        let http_client = reqwest::Client::new();
        Self {
            cache: HashMap::default(),
            picker,
            http_client,
            tx: sender,
        }
    }

    pub fn get_mut(&mut self, url: &str) -> Option<&mut AppImage> {
        if !self.cache.contains_key(url) {
            self.cache.insert(url.to_owned(), CachedImage::Loading);

            let url = url.to_owned();
            let picker = self.picker.clone();
            let http_client = self.http_client.clone();
            let tx = self.tx.clone();

            tokio::spawn(async move {
                let image = fetch_image(&picker, &url, &http_client).await;
                let _ = tx.send(ImageLoaded { url, image });
            });
        }

        match self.cache.get_mut(url) {
            Some(CachedImage::Ready(image)) => Some(image),
            Some(CachedImage::Loading | CachedImage::Failed) | None => None,
        }
    }

    pub fn insert(&mut self, message: ImageLoaded) {
        let state = match message.image {
            Some(image) => CachedImage::Ready(image),
            None => CachedImage::Failed,
        };

        self.cache.insert(message.url, state);
    }
}

async fn fetch_image(
    picker: &Picker,
    image_url: &str,
    http_client: &reqwest::Client,
) -> Option<AppImage> {
    let response = http_client.get(image_url).send().await.ok()?;
    let img_bytes = response.bytes().await.ok()?;
    let picker = picker.clone();

    tokio::task::spawn_blocking(move || {
        let image = load_from_memory(&img_bytes).ok()?;
        let ratio = image.width().to_f32()? / image.height().to_f32()?;
        let protocol = picker.new_resize_protocol(image);

        Some(AppImage { protocol, ratio })
    })
    .await
    .ok()?
}
